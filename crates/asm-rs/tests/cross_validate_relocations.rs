#![cfg(not(target_arch = "wasm32"))]
//! Cross-validation of **linker-applied relocations**.
//!
//! The other cross-validation suites assemble one standalone instruction at a
//! time, so they only ever exercise the encoders. Everything the linker does —
//! resolving a label to an address and packing the resulting displacement into
//! the instruction word — is invisible to them.
//!
//! That is not a hypothetical gap. Three separate relocation bugs shipped
//! behind fully-green encoder cross-validation:
//!
//! - RISC-V `RvJal20` never shifted `imm[20]` into bit 31, so every backward
//!   `jal` encoded a large *positive* displacement.
//! - Thumb-2 `BL`/`B.W` read `I1`/`I2` from the wrong bits, silently halving
//!   the reachable branch distance.
//! - The RISC-V `AUIPC`+`JALR` pair had no range check and truncated far
//!   targets to a plausible-looking wrong address.
//!
//! Each is invisible near zero displacement and only appears at distance, so
//! these tests sweep a range of displacements — including the sign boundary and
//! the ends of each encoding's range — and check the address an *independent*
//! decoder computes, not the bytes we happened to emit.

use asm_rs::{Arch, Assembler};

/// Assemble `body` at `base`, with `target` predefined as an external label.
fn assemble_branch(arch: Arch, body: &str, base: u64, target: u64) -> Vec<u8> {
    let mut asm = Assembler::new(arch);
    asm.base_address(base);
    asm.define_external("target", target);
    asm.emit(body)
        .unwrap_or_else(|e| panic!("asm-rs failed on `{body}` (target {target:#x}): {e}"));
    asm.finish()
        .unwrap_or_else(|e| panic!("asm-rs link failed on `{body}` (target {target:#x}): {e}"))
        .into_bytes()
}

/// Displacements that bracket the interesting cases for a branch whose
/// encoding has `range` bytes of reach in each direction: zero, the sign
/// boundary, powers of two on both sides, and the extremes.
///
/// The extremes stay a few units inside `range` because several encodings
/// measure their displacement from a PC that is 4 or 8 bytes past the
/// instruction, which shifts the reachable window.
fn sweep(range: i64, align: i64) -> Vec<i64> {
    let margin = align * 4;
    let mut v = vec![0, align, -align, range - margin, -(range - margin)];
    let mut step = align * 2;
    while step < range - margin {
        v.push(step);
        v.push(-step);
        step *= 4;
    }
    v
}

// ============================================================================
// x86-64 — decoded with iced-x86
// ============================================================================

#[cfg(feature = "x86_64")]
mod x86_64 {
    use super::*;
    use iced_x86::{Decoder, DecoderOptions};

    /// Decode the single instruction at `base` and return the branch target
    /// address iced-x86 computes for it.
    fn decoded_branch_target(bytes: &[u8], base: u64) -> u64 {
        let mut decoder = Decoder::with_ip(64, bytes, base, DecoderOptions::NONE);
        let insn = decoder.decode();
        assert!(!insn.is_invalid(), "iced-x86 rejected {bytes:02X?}");
        insn.near_branch_target()
    }

    fn check(mnemonic: &str, disp: i64) {
        let base = 0x1_0000u64;
        // The branch is the first thing at `base`, so the target address is
        // simply base + disp (disp is measured from the instruction start).
        let target = base.wrapping_add(disp as u64);
        let bytes = assemble_branch(Arch::X86_64, &format!("{mnemonic} target"), base, target);
        assert_eq!(
            decoded_branch_target(&bytes, base),
            target,
            "{mnemonic} disp={disp:#x} encoded {bytes:02X?}"
        );
    }

    #[test]
    fn jmp_rel32_targets_round_trip() {
        for disp in super::sweep(1 << 30, 1) {
            check("jmp", disp);
        }
    }

    #[test]
    fn jcc_rel32_targets_round_trip() {
        for mnemonic in ["je", "jne", "jl", "jge", "jbe", "ja"] {
            for disp in super::sweep(1 << 20, 1) {
                check(mnemonic, disp);
            }
        }
    }

    #[test]
    fn call_rel32_targets_round_trip() {
        for disp in super::sweep(1 << 28, 1) {
            check("call", disp);
        }
    }

    /// A backward branch inside a single assembly unit — exercises label
    /// resolution and relaxation together, rather than an external symbol.
    #[test]
    fn backward_branch_relaxes_and_resolves() {
        for pad in [0usize, 10, 100, 126, 127, 128, 200, 5000] {
            let mut src = String::from("top:\n");
            for _ in 0..pad {
                src.push_str("nop\n");
            }
            src.push_str("jmp top\n");

            let mut asm = Assembler::new(Arch::X86_64);
            asm.base_address(0x2000);
            asm.emit(&src).unwrap();
            let bytes = asm.finish().unwrap().into_bytes();

            let branch_at = 0x2000u64 + pad as u64;
            assert_eq!(
                decoded_branch_target(&bytes[pad..], branch_at),
                0x2000,
                "pad={pad} bytes={:02X?}",
                &bytes[pad..]
            );
        }
    }
}

// ============================================================================
// AArch64 — decoded with bad64
// ============================================================================

#[cfg(feature = "aarch64")]
mod aarch64 {
    use super::*;
    use bad64::{Imm, Operand};

    /// Decode the word at `base` and return the address of its `Label` operand.
    fn decoded_label(bytes: &[u8], base: u64) -> u64 {
        let word = u32::from_le_bytes(bytes[..4].try_into().unwrap());
        let insn = bad64::decode(word, base)
            .unwrap_or_else(|e| panic!("bad64 rejected {word:#010x}: {e:?}"));
        for op in insn.operands() {
            if let Operand::Label(imm) = op {
                return match *imm {
                    Imm::Signed(v) => v as u64,
                    Imm::Unsigned(v) => v,
                };
            }
        }
        panic!("no label operand in `{insn}` ({word:#010x})");
    }

    fn check(source_fmt: &str, range: i64) {
        let base = 0x10_0000u64;
        for disp in super::sweep(range, 4) {
            let target = base.wrapping_add(disp as u64);
            let bytes = assemble_branch(Arch::Aarch64, source_fmt, base, target);
            assert_eq!(
                decoded_label(&bytes, base),
                target,
                "`{source_fmt}` disp={disp:#x} encoded {bytes:02X?}"
            );
        }
    }

    /// B/BL use a 26-bit signed word offset: ±128 MiB.
    #[test]
    fn b_and_bl_jump26_round_trip() {
        check("b target", 1 << 27);
        check("bl target", 1 << 27);
    }

    /// B.cond / CBZ / CBNZ use a 19-bit signed word offset: ±1 MiB.
    #[test]
    fn cond_branch19_round_trip() {
        check("b.eq target", 1 << 20);
        check("b.ne target", 1 << 20);
        check("cbz x0, target", 1 << 20);
        check("cbnz w3, target", 1 << 20);
    }

    /// TBZ / TBNZ use a 14-bit signed word offset: ±32 KiB.
    ///
    /// These relax to an inverted test plus a `B` when the target is out of
    /// reach, so the sweep stays inside the short form's range.
    #[test]
    fn test_branch14_round_trip() {
        check("tbz x0, #3, target", 1 << 15);
        check("tbnz x1, #40, target", 1 << 15);
    }

    /// Shifted-register, shifted-immediate and unscaled load/store operands,
    /// checked against bad64's rendering rather than against our own bytes.
    #[test]
    fn shifted_and_unscaled_operands_round_trip() {
        let cases: &[(&str, &str)] = &[
            ("add x0, x1, x2, lsl #3", "add x0, x1, x2, lsl #0x3"),
            ("sub w0, w1, w2, asr #7", "sub w0, w1, w2, asr #0x7"),
            ("and x0, x1, x2, ror #12", "and x0, x1, x2, ror #0xc"),
            ("orr x3, x4, x5, lsr #1", "orr x3, x4, x5, lsr #0x1"),
            ("ands x0, x1, x2, lsl #4", "ands x0, x1, x2, lsl #0x4"),
            ("add x0, x1, #0x1, lsl #12", "add x0, x1, #0x1, lsl #0xc"),
            ("movk x0, #0xABCD, lsl #32", "movk x0, #0xabcd, lsl #0x20"),
            ("ror x0, x1, #4", "ror x0, x1, #0x4"),
            ("ldur x0, [x1, #-16]", "ldur x0, [x1, #0xfffffffffffffff0]"),
            ("stur w0, [x1, #4]", "stur w0, [x1, #0x4]"),
            (
                "ldursw x0, [x1, #-4]",
                "ldursw x0, [x1, #0xfffffffffffffffc]",
            ),
        ];
        for (source, expected) in cases {
            let bytes = assemble_branch(Arch::Aarch64, source, 0, 0);
            let word = u32::from_le_bytes(bytes[..4].try_into().unwrap());
            let insn = bad64::decode(word, 0)
                .unwrap_or_else(|e| panic!("bad64 rejected `{source}` ({word:#010x}): {e:?}"));
            assert_eq!(
                insn.to_string(),
                *expected,
                "`{source}` encoded {word:#010x}"
            );
        }
    }

    /// Operand combinations that are not encodable must be diagnosed rather
    /// than silently dropped — an ignored shift computes something else.
    #[test]
    fn unencodable_shifts_are_rejected() {
        for source in [
            "add x0, x1, x2, ror #3",  // ROR is reserved for add/sub
            "add w0, w1, w2, lsl #32", // amount exceeds W-register width
            "add x0, x1, #1, lsl #4",  // only lsl #0 / #12 on add/sub imm
            "movz x0, #1, lsl #8",     // move-wide shift must be a lane
            "movz w0, #1, lsl #32",    // out of range for a W register
            "ldur x0, [x1, #256]",     // unscaled offset is 9-bit signed
        ] {
            assert!(
                Assembler::new(Arch::Aarch64).encode_one(source).is_err(),
                "`{source}` should not encode"
            );
        }
    }

    /// ADR uses a 21-bit signed *byte* offset: ±1 MiB, not word-scaled.
    #[test]
    fn adr21_round_trip() {
        let base = 0x10_0000u64;
        for disp in super::sweep(1 << 20, 1) {
            let target = base.wrapping_add(disp as u64);
            let bytes = assemble_branch(Arch::Aarch64, "adr x0, target", base, target);
            assert_eq!(
                decoded_label(&bytes, base),
                target,
                "adr disp={disp:#x} encoded {bytes:02X?}"
            );
        }
    }
}

// ============================================================================
// RISC-V — decoded with riscv-decode
// ============================================================================

#[cfg(feature = "riscv")]
mod riscv {
    use super::*;
    use riscv_decode::Instruction;

    /// Sign-extend the low `bits` of `v`.
    fn sext(v: u32, bits: u32) -> i64 {
        let shift = 32 - bits;
        i64::from(((v << shift) as i32) >> shift)
    }

    /// Decode the word at offset 0 and return its PC-relative displacement.
    fn decoded_displacement(bytes: &[u8]) -> i64 {
        let word = u32::from_le_bytes(bytes[..4].try_into().unwrap());
        match riscv_decode::decode(word)
            .unwrap_or_else(|e| panic!("riscv-decode rejected {word:#010x}: {e:?}"))
        {
            Instruction::Jal(j) => sext(j.imm(), 21),
            Instruction::Beq(b)
            | Instruction::Bne(b)
            | Instruction::Blt(b)
            | Instruction::Bge(b)
            | Instruction::Bltu(b)
            | Instruction::Bgeu(b) => sext(b.imm(), 13),
            other => panic!("unexpected instruction {other:?} from {word:#010x}"),
        }
    }

    fn check(source: &str, range: i64) {
        let base = 0x10_0000u64;
        for disp in super::sweep(range, 2) {
            let target = base.wrapping_add(disp as u64);
            let bytes = assemble_branch(Arch::Rv64, source, base, target);
            assert_eq!(
                decoded_displacement(&bytes),
                disp,
                "`{source}` disp={disp:#x} encoded {bytes:02X?}"
            );
        }
    }

    /// JAL uses a 21-bit signed offset: ±1 MiB.
    ///
    /// This is the regression that motivated the file: `imm[20]` belongs in bit
    /// 31 of the word, and dropping the shift turned every negative
    /// displacement into a large positive one.
    #[test]
    fn jal20_round_trip() {
        check("jal ra, target", 1 << 20);
        check("jal zero, target", 1 << 20);
    }

    /// B-type branches use a 13-bit signed offset: ±4 KiB.
    #[test]
    fn branch12_round_trip() {
        for source in [
            "beq a0, a1, target",
            "bne a0, a1, target",
            "blt a0, a1, target",
            "bge a0, a1, target",
            "bltu a0, a1, target",
            "bgeu a0, a1, target",
        ] {
            check(source, 1 << 12);
        }
    }

    /// `call` expands to AUIPC+JALR, which reaches ±2 GiB. Decode both halves
    /// and recombine them the way the hardware does: AUIPC adds a
    /// sign-extended `hi20 << 12` to the PC, then JALR adds a sign-extended
    /// 12-bit immediate.
    #[test]
    fn auipc_jalr_pair_round_trip() {
        let base = 0x10_0000u64;
        for disp in [
            0i64,
            4,
            -4,
            0x800,
            -0x800,
            0x1000,
            -0x1000,
            0x7FFF_F000,
            -0x7FFF_F000,
        ] {
            let target = base.wrapping_add(disp as u64);
            let bytes = assemble_branch(Arch::Rv64, "call target", base, target);
            assert_eq!(bytes.len(), 8, "call must expand to AUIPC+JALR");

            let auipc = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
            let jalr = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
            let hi20 = i64::from((auipc & 0xFFFF_F000) as i32); // already at bit 12, sign-extended
            let lo12 = sext(jalr >> 20, 12);

            assert_eq!(
                base.wrapping_add((hi20 + lo12) as u64),
                target,
                "call disp={disp:#x} encoded {bytes:02X?}"
            );
        }
    }

    /// Beyond ±2 GiB the pair cannot represent the target; the linker must say
    /// so rather than truncate to a plausible wrong address.
    #[test]
    fn auipc_jalr_rejects_unreachable_target() {
        let mut asm = Assembler::new(Arch::Rv64);
        asm.define_external("target", 0x8000_0000);
        asm.emit("call target").unwrap();
        assert!(
            asm.finish().is_err(),
            "a target past ±2 GiB must be diagnosed, not truncated"
        );
    }
}

// ============================================================================
// ARM / Thumb — decoded by unpacking the documented field layout
// ============================================================================

#[cfg(feature = "arm")]
mod arm {
    use super::*;

    /// Recover the byte displacement from an A32 `B`/`BL` word, per the Arm
    /// ARM: `imm24` is a signed word offset relative to PC = instruction + 8.
    fn a32_branch_displacement(bytes: &[u8]) -> i64 {
        let word = u32::from_le_bytes(bytes[..4].try_into().unwrap());
        let imm24 = word & 0x00FF_FFFF;
        let signed = i64::from(((imm24 << 8) as i32) >> 8);
        signed * 4 + 8
    }

    #[test]
    fn a32_b_and_bl_branch24_round_trip() {
        let base = 0x10_0000u64;
        for source in ["b target", "bl target"] {
            for disp in super::sweep(1 << 25, 4) {
                let target = base.wrapping_add(disp as u64);
                let bytes = assemble_branch(Arch::Arm, source, base, target);
                assert_eq!(
                    a32_branch_displacement(&bytes),
                    disp,
                    "`{source}` disp={disp:#x} encoded {bytes:02X?}"
                );
            }
        }
    }

    /// Recover the byte displacement from a Thumb-2 `BL` (T1) or `B.W` (T4)
    /// halfword pair. Both reconstruct the offset as
    /// `SignExtend(S:I1:I2:imm10:imm11:'0')` with `I1 = NOT(J1 EOR S)` and
    /// `I2 = NOT(J2 EOR S)`, relative to PC = instruction + 4.
    fn thumb32_branch_displacement(bytes: &[u8]) -> i64 {
        let hw1 = u16::from_le_bytes([bytes[0], bytes[1]]);
        let hw2 = u16::from_le_bytes([bytes[2], bytes[3]]);
        let s = u32::from((hw1 >> 10) & 1);
        let imm10 = u32::from(hw1 & 0x3FF);
        let j1 = u32::from((hw2 >> 13) & 1);
        let j2 = u32::from((hw2 >> 11) & 1);
        let imm11 = u32::from(hw2 & 0x7FF);
        let i1 = !(j1 ^ s) & 1;
        let i2 = !(j2 ^ s) & 1;
        let raw = (s << 24) | (i1 << 23) | (i2 << 22) | (imm10 << 12) | (imm11 << 1);
        i64::from(((raw << 7) as i32) >> 7) + 4
    }

    /// The `I1`/`I2` bits only diverge from the sign bit past ±4 MiB, which is
    /// exactly why reading them from the wrong position went unnoticed — so the
    /// sweep has to reach the full ±16 MiB range.
    #[test]
    fn thumb_bl_branch_round_trip() {
        let base = 0x10_0000u64;
        for disp in super::sweep(1 << 24, 2) {
            let target = base.wrapping_add(disp as u64);
            let bytes = assemble_branch(Arch::Thumb, "bl target", base, target);
            assert_eq!(bytes.len(), 4, "Thumb BL is a 32-bit instruction");
            assert_eq!(
                thumb32_branch_displacement(&bytes),
                disp,
                "bl disp={disp:#x} encoded {bytes:02X?}"
            );
        }
    }
}
