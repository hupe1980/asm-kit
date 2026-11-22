#![no_main]
//! Differential fuzzing: assemble, then decode with an independent decoder and
//! check that the decoder agrees about where the branch goes.
//!
//! The other fuzz targets only assert that the assembler never panics. That is
//! worth having, but it cannot catch the failure mode that actually matters
//! for an assembler: emitting well-formed machine code that means something
//! different from the source. Every relocation bug found in this codebase was
//! of exactly that kind — a `jal` whose sign bit was dropped, a Thumb `bl`
//! whose reach was halved, an `auipc`/`jalr` pair that truncated. All three
//! produce bytes a decoder happily accepts.
//!
//! So this target generates a *branch to a known address* and compares the
//! address the reference decoder computes against the one that was asked for.
//! A disagreement is a miscompilation.
//!
//! Structured input: the fuzzer picks an architecture, a branch form and a
//! displacement, rather than mutating assembly text. Text mutation almost never
//! produces a valid far branch, so it would never reach the interesting code.

use libfuzzer_sys::fuzz_target;

use asm_kit::{Arch, Assembler};

/// One generated case: which branch to emit and how far away its target is.
struct Case {
    arch: Arch,
    form: usize,
    disp: i64,
}

impl Case {
    /// Build a case from raw fuzzer bytes.
    fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 10 {
            return None;
        }
        let arch = match data[0] % 4 {
            0 => Arch::X86_64,
            1 => Arch::Aarch64,
            2 => Arch::Rv64,
            _ => Arch::Thumb,
        };
        let form = data[1] as usize;
        // Take a full 64-bit displacement so the fuzzer can reach the extremes
        // of every encoding, including just past each range boundary.
        let disp = i64::from_le_bytes([
            data[2], data[3], data[4], data[5], data[6], data[7], data[8], data[9],
        ]);
        Some(Case { arch, form, disp })
    }

    /// The instruction text for this case.
    fn source(&self) -> &'static str {
        const X86: &[&str] = &["jmp target", "call target", "je target", "jne target"];
        const A64: &[&str] = &["b target", "bl target", "b.eq target", "cbz x0, target"];
        const RV: &[&str] = &["jal ra, target", "beq a0, a1, target", "jal zero, target"];
        const THUMB: &[&str] = &["bl target", "b target"];
        let table: &[&str] = match self.arch {
            Arch::X86_64 => X86,
            Arch::Aarch64 => A64,
            Arch::Rv64 => RV,
            _ => THUMB,
        };
        table[self.form % table.len()]
    }
}

/// Recover the branch displacement an independent decoder sees, or `None` if
/// this form is not one the target decodes.
///
/// `base` is the address of `bytes[0]`, and the displacement returned is
/// relative to it.
fn decoded_displacement(arch: Arch, bytes: &[u8], base: u64) -> Option<i64> {
    match arch {
        Arch::X86_64 => {
            let mut dec = iced_x86::Decoder::with_ip(64, bytes, base, iced_x86::DecoderOptions::NONE);
            let insn = dec.decode();
            if insn.is_invalid() {
                return None;
            }
            Some(insn.near_branch_target() as i64 - base as i64)
        }
        Arch::Aarch64 => {
            let word = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?);
            let insn = bad64::decode(word, base).ok()?;
            for op in insn.operands() {
                if let bad64::Operand::Label(imm) = op {
                    let addr = match *imm {
                        bad64::Imm::Signed(v) => v,
                        bad64::Imm::Unsigned(v) => v as i64,
                    };
                    return Some(addr - base as i64);
                }
            }
            None
        }
        Arch::Rv64 => {
            let word = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?);
            let sext = |v: u32, bits: u32| -> i64 {
                let shift = 32 - bits;
                i64::from(((v << shift) as i32) >> shift)
            };
            match riscv_decode::decode(word).ok()? {
                riscv_decode::Instruction::Jal(j) => Some(sext(j.imm(), 21)),
                riscv_decode::Instruction::Beq(b) => Some(sext(b.imm(), 13)),
                _ => None,
            }
        }
        // Thumb-2 BL/B.W, unpacked per the Arm ARM: the offset is
        // SignExtend(S:I1:I2:imm10:imm11:'0') relative to PC = instruction + 4.
        _ => {
            if bytes.len() < 4 {
                return None;
            }
            let hw1 = u16::from_le_bytes([bytes[0], bytes[1]]);
            let hw2 = u16::from_le_bytes([bytes[2], bytes[3]]);
            // Only the 32-bit forms are unpacked here.
            if hw1 & 0xF800 != 0xF000 {
                return None;
            }
            let s = u32::from((hw1 >> 10) & 1);
            let imm10 = u32::from(hw1 & 0x3FF);
            let j1 = u32::from((hw2 >> 13) & 1);
            let j2 = u32::from((hw2 >> 11) & 1);
            let imm11 = u32::from(hw2 & 0x7FF);
            let i1 = !(j1 ^ s) & 1;
            let i2 = !(j2 ^ s) & 1;
            let raw = (s << 24) | (i1 << 23) | (i2 << 22) | (imm10 << 12) | (imm11 << 1);
            Some(i64::from(((raw << 7) as i32) >> 7) + 4)
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let Some(case) = Case::from_bytes(data) else {
        return;
    };

    // A base address far enough from zero that a negative displacement is
    // still a valid address.
    const BASE: u64 = 0x4000_0000;
    let Some(target) = BASE.checked_add_signed(case.disp) else {
        return;
    };

    let mut asm = Assembler::new(case.arch);
    asm.base_address(BASE);
    asm.define_external("target", target);
    if asm.emit(case.source()).is_err() {
        return;
    }
    // Out-of-range targets *must* be rejected; that is the contract, and a
    // rejection is a pass, not a miss.
    let Ok(result) = asm.finish() else {
        return;
    };
    let bytes = result.bytes();

    // A branch too far for its short form is relaxed into a sequence: on
    // AArch64 a `cbz` past ±1 MiB becomes an inverted `cbnz` over an
    // unconditional `b`. The instruction that actually reaches the target is
    // then the last one, so decode from there rather than from the start.
    let insn_len = match case.arch {
        Arch::X86_64 => bytes.len(), // variable length; iced decodes the first
        _ => 4,
    };
    if bytes.len() < insn_len {
        return;
    }
    let tail = bytes.len() - insn_len;
    let Some(decoded) = decoded_displacement(case.arch, &bytes[tail..], BASE + tail as u64) else {
        return;
    };
    // `decoded` is relative to the tail instruction; re-base it on the branch.
    let decoded = decoded + tail as i64;

    assert_eq!(
        decoded, case.disp,
        "miscompiled `{}` for {:?}: asked for {:#x}, decoder reads {:#x} from {:02X?}",
        case.source(),
        case.arch,
        case.disp,
        decoded,
        bytes,
    );
});
