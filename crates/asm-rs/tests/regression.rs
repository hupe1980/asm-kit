//! Regression tests for bug fixes.
//!
//! Each test documents a specific bug that was found and fixed, ensuring the
//! fix is never accidentally reverted.

use asm_rs::{assemble, Arch, AsmError, Assembler, OptLevel, ResourceLimits};

/// Regression: x86-32 INC/DEC short forms (0x40–0x4F) must use the
/// single-byte opcode, not the ModR/M form used in x86-64 (where 0x40–0x4F
/// are REX prefixes).
#[test]
#[cfg(feature = "x86")]
fn x86_32_inc_dec_short_forms() {
    let mut asm = Assembler::new(Arch::X86);
    asm.emit("inc eax\ndec ebx").unwrap();
    let result = asm.finish().unwrap();
    // INC EAX = 0x40, DEC EBX = 0x4B (short forms)
    assert_eq!(result.bytes(), &[0x40, 0x4B]);
}

/// Regression: `mov reg, 0` → `xor reg, reg` clobbers FLAGS, so it must only
/// happen at [`OptLevel::Aggressive`]. At the default level the instruction is
/// emitted as written, keeping sequences like `cmp` / `mov` / `sete` correct.
#[test]
#[cfg(feature = "x86_64")]
fn x86_64_zero_idiom_requires_aggressive_opt_level() {
    let mut asm = Assembler::new(Arch::X86_64);
    asm.emit("mov ecx, 0").unwrap();
    // Default (OptLevel::Size): MOV ECX, imm32 = B9 00 00 00 00
    assert_eq!(asm.finish().unwrap().bytes(), &[0xB9, 0, 0, 0, 0]);

    let mut asm = Assembler::new(Arch::X86_64);
    asm.optimize(OptLevel::Aggressive);
    asm.emit("mov ecx, 0").unwrap();
    // Opted in: XOR ECX, ECX = 31 C9 (2 bytes, zero idiom)
    assert_eq!(asm.finish().unwrap().bytes(), &[0x31, 0xC9]);
}

/// Regression: the RISC-V J-type immediate places `imm[20]` in bit 31 of the
/// instruction word. Failing to shift it left by 11 dropped the sign bit, so
/// every backward `jal` — and any forward one past 512 KiB — encoded a wrong
/// (positive, far) target with no diagnostic.
#[test]
#[cfg(feature = "riscv")]
fn riscv_jal_backward_sets_sign_bit() {
    let code = assemble("target:\nnop\nnop\njal ra, target", Arch::Rv64).unwrap();
    let word = u32::from_le_bytes([code[8], code[9], code[10], code[11]]);

    // Decode the J-type immediate back out and sign-extend it.
    let imm = ((word >> 31) & 1) << 20
        | ((word >> 21) & 0x3FF) << 1
        | ((word >> 20) & 1) << 11
        | ((word >> 12) & 0xFF) << 12;
    let disp = ((imm << 11) as i32) >> 11;
    assert_eq!(disp, -8, "jal encoded {word:#010x}");
}

/// Regression: in the Thumb-2 `BL`/`B.W` encodings the `J1`/`J2` bits derive
/// from `I1` (bit 22) and `I2` (bit 21) of the halfword-scaled offset, not from
/// the sign bit. Reading the wrong bits is invisible within ±4 MiB — where
/// sign extension makes `I1 == I2 == S` — and silently halves the branch
/// distance beyond it.
#[test]
#[cfg(feature = "arm")]
fn thumb_bl_far_target_encodes_i1_i2() {
    let target: i64 = 0x0080_0000; // 8 MiB — needs I1 != S
    let mut asm = Assembler::new(Arch::Thumb);
    asm.define_external("far", target as u64);
    asm.emit("bl far").unwrap();
    let code = asm.finish().unwrap().into_bytes();

    let hw1 = u16::from_le_bytes([code[0], code[1]]);
    let hw2 = u16::from_le_bytes([code[2], code[3]]);
    let s = u32::from((hw1 >> 10) & 1);
    let imm10 = u32::from(hw1 & 0x3FF);
    let j1 = u32::from((hw2 >> 13) & 1);
    let j2 = u32::from((hw2 >> 11) & 1);
    let imm11 = u32::from(hw2 & 0x7FF);
    let i1 = !(j1 ^ s) & 1;
    let i2 = !(j2 ^ s) & 1;
    let raw = (s << 24) | (i1 << 23) | (i2 << 22) | (imm10 << 12) | (imm11 << 1);
    let disp = ((raw as i32) << 7) >> 7;

    // PC is the instruction address + 4 in Thumb.
    assert_eq!(i64::from(disp), target - 4);
}

/// Regression: the RISC-V `AUIPC`+`JALR` pair reaches ±2 GiB. Without a range
/// check a farther target truncated to 32 bits and produced a valid-looking
/// jump to the wrong address instead of an error.
#[test]
#[cfg(feature = "riscv")]
fn riscv_auipc_jalr_rejects_out_of_range_target() {
    let mut asm = Assembler::new(Arch::Rv64);
    asm.define_external("far", 0xFFFF_FFFF_0000_0000);
    asm.emit("call far").unwrap();
    assert!(matches!(
        asm.finish(),
        Err(AsmError::BranchOutOfRange { .. })
    ));
}

/// Regression: `.org` and `.align` request padding that is unrelated to the
/// size of the source text. The output ceiling has to be enforced during
/// layout — checking it only after the bytes exist meant a two-line input
/// could reserve terabytes first.
#[test]
#[cfg(feature = "x86_64")]
fn org_padding_is_bounded_before_allocation() {
    let mut asm = Assembler::new(Arch::X86_64);
    asm.emit("nop").unwrap();
    asm.emit(".org 0xFFFFFFFFFFF").unwrap();
    assert!(matches!(
        asm.finish(),
        Err(AsmError::ResourceLimitExceeded { .. })
    ));

    let mut asm = Assembler::new(Arch::X86_64);
    asm.emit("nop").unwrap();
    asm.emit(".align 0x20000000").unwrap();
    assert!(matches!(
        asm.finish(),
        Err(AsmError::ResourceLimitExceeded { .. })
    ));
}

/// Regression: preprocessor iteration and recursion counters bound how often
/// expansion loops, not how much text it emits. A single `.rept` around a
/// large body stayed well under the iteration limit while producing hundreds
/// of megabytes, so expansion size is metered separately.
#[test]
#[cfg(feature = "x86_64")]
fn preprocessor_expansion_size_is_bounded() {
    let body = "nop\n".repeat(2000); // 8 KiB body
    let src = format!(".rept 50000\n{body}.endr\n"); // would be ~400 MiB

    let mut asm = Assembler::new(Arch::X86_64);
    asm.limits(ResourceLimits {
        max_expanded_bytes: 1024 * 1024,
        ..Default::default()
    });
    assert!(matches!(
        asm.emit(&src),
        Err(AsmError::ResourceLimitExceeded { .. })
    ));
}

/// Regression: `#` is the immediate prefix in UAL (`mov r0, #1`) but a comment
/// character in GNU-as/Intel syntax. Treating it as a comment unconditionally
/// made the canonical ARM/Thumb/AArch64 spelling of every immediate a syntax
/// error, so the lexer now follows the selected dialect.
#[test]
#[cfg(all(feature = "arm", feature = "aarch64"))]
fn ual_hash_immediate_prefix_is_accepted() {
    // AArch64
    assert_eq!(
        assemble("mov x0, #1", Arch::Aarch64).unwrap(),
        assemble("mov x0, 1", Arch::Aarch64).unwrap()
    );
    assert_eq!(
        assemble("ldr x0, [x1, #16]", Arch::Aarch64).unwrap(),
        assemble("ldr x0, [x1, 16]", Arch::Aarch64).unwrap()
    );
    // ARM A32 and Thumb
    assert_eq!(
        assemble("add r0, r1, #8", Arch::Arm).unwrap(),
        assemble("add r0, r1, 8", Arch::Arm).unwrap()
    );
    assert_eq!(
        assemble("movs r0, #1", Arch::Thumb).unwrap(),
        assemble("movs r0, 1", Arch::Thumb).unwrap()
    );

    // `@` and `//` are the comment characters in UAL...
    assert_eq!(
        assemble("mov x0, #1 @ set to one", Arch::Aarch64).unwrap(),
        assemble("mov x0, #1", Arch::Aarch64).unwrap()
    );
    assert_eq!(
        assemble("mov x0, #1 // set to one", Arch::Aarch64).unwrap(),
        assemble("mov x0, #1", Arch::Aarch64).unwrap()
    );
    // ...while `#` still starts a comment for the Intel-syntax targets.
    assert_eq!(
        assemble("nop # comment", Arch::X86_64).unwrap(),
        assemble("nop", Arch::X86_64).unwrap()
    );
}

/// Regression: on AArch64 register number 31 means SP in some encodings and
/// XZR in others. Add/sub with a register operand always used the
/// shifted-register form, where 31 is XZR — so `add sp, sp, x0`, the standard
/// stack adjustment, assembled cleanly as `add xzr, xzr, x0` and discarded its
/// result. SP operands now select the extended-register form, where 31 is SP.
#[test]
#[cfg(feature = "aarch64")]
fn aarch64_sp_uses_encoding_that_can_name_it() {
    // ADD (extended register), option=UXTX, imm3=0 — decodes as `add sp, sp, x0`.
    assert_eq!(
        assemble("add sp, sp, x0", Arch::Aarch64).unwrap(),
        0x8B2063FFu32.to_le_bytes()
    );
    assert_eq!(
        assemble("sub sp, sp, x8", Arch::Aarch64).unwrap(),
        0xCB2863FFu32.to_le_bytes()
    );
    assert_eq!(
        assemble("add x0, sp, x1", Arch::Aarch64).unwrap(),
        0x8B2163E0u32.to_le_bytes()
    );
    // An explicit shift keeps its amount in the extend encoding's imm3 field.
    assert_eq!(
        assemble("add sp, x1, x2, lsl #2", Arch::Aarch64).unwrap(),
        0x8B22683Fu32.to_le_bytes()
    );

    // `MOV` to or from SP is an `ADD Rd, Rn, #0` alias — the usual
    // `ORR Rd, XZR, Rm` form cannot name SP at all.
    assert_eq!(
        assemble("mov sp, x0", Arch::Aarch64).unwrap(),
        0x9100001Fu32.to_le_bytes()
    );
    assert_eq!(
        assemble("mov x0, sp", Arch::Aarch64).unwrap(),
        0x910003E0u32.to_le_bytes()
    );
    // Without SP, MOV still uses the ORR alias.
    assert_eq!(
        assemble("mov x0, x1", Arch::Aarch64).unwrap(),
        0xAA0103E0u32.to_le_bytes()
    );
    // XZR must keep meaning XZR.
    assert_eq!(
        assemble("mov xzr, x0", Arch::Aarch64).unwrap(),
        0xAA0003FFu32.to_le_bytes()
    );

    // The flag-setting form cannot write to SP; saying so beats encoding XZR.
    assert!(assemble("adds sp, sp, x1", Arch::Aarch64).is_err());
}

/// Regression: register ranges (`{r0-r7}`) are the usual way to spell a
/// push/pop/LDM/STM list; only the fully-enumerated form was accepted.
#[test]
#[cfg(feature = "arm")]
fn register_ranges_expand_in_lists() {
    assert_eq!(
        assemble("push {r0-r7}", Arch::Thumb).unwrap(),
        assemble("push {r0, r1, r2, r3, r4, r5, r6, r7}", Arch::Thumb).unwrap()
    );
    assert_eq!(
        assemble("stmdb sp!, {r4-r11, lr}", Arch::Arm).unwrap(),
        assemble(
            "stmdb sp!, {r4, r5, r6, r7, r8, r9, r10, r11, lr}",
            Arch::Arm
        )
        .unwrap()
    );
    // An inverted range is a mistake, not an empty list.
    assert!(assemble("push {r4-r0}", Arch::Arm).is_err());
}

/// Regression: the barrel shifter is a defining feature of ARM, and the
/// encoders supported it — but the parser had no way to produce a shifted
/// operand, so `add r0, r1, r2, lsl #3` was a syntax error and the encoder
/// path was unreachable.
#[test]
#[cfg(feature = "arm")]
fn arm_barrel_shift_operands() {
    // Immediate and register shift amounts.
    assert_eq!(
        assemble("add r0, r1, r2, lsl #3", Arch::Arm).unwrap(),
        0xE081_0182u32.to_le_bytes()
    );
    assert_eq!(
        assemble("add r0, r1, r2, lsl r3", Arch::Arm).unwrap(),
        0xE081_0312u32.to_le_bytes()
    );
    // The shift mnemonics are MOV aliases.
    assert_eq!(
        assemble("lsl r0, r1, #4", Arch::Arm).unwrap(),
        assemble("mov r0, r1, lsl #4", Arch::Arm).unwrap()
    );
    assert_eq!(
        assemble("rrx r0, r1", Arch::Arm).unwrap(),
        assemble("mov r0, r1, rrx", Arch::Arm).unwrap()
    );
    // A shift with no amount must not be silently dropped — that would encode
    // a different computation from the one written.
    assert!(assemble("add r0, r1, r2, lsl", Arch::Arm).is_err());
    assert!(assemble("add r0, r1, r2, lsl #32", Arch::Arm).is_err());
}

/// Regression: PC-relative branch encodings store their displacement
/// pre-scaled — AArch64 and ARM shift right by 2, Thumb and RISC-V by 1 — so a
/// misaligned target had its low bits shifted off and branched somewhere else.
/// The result was well-formed machine code a decoder accepts, which is why
/// only differential fuzzing caught it.
#[test]
fn misaligned_branch_targets_are_rejected() {
    let cases: &[(Arch, &str, u64, u8)] = &[
        #[cfg(feature = "aarch64")]
        (Arch::Aarch64, "b target", 0x4000_0131, 4),
        #[cfg(feature = "aarch64")]
        (Arch::Aarch64, "bl target", 0x4000_0002, 4),
        #[cfg(feature = "aarch64")]
        (Arch::Aarch64, "b.eq target", 0x4000_0002, 4),
        #[cfg(feature = "arm")]
        (Arch::Arm, "b target", 0x4000_0002, 4),
        #[cfg(feature = "arm")]
        (Arch::Thumb, "bl target", 0x4000_0001, 2),
        #[cfg(feature = "riscv")]
        (Arch::Rv64, "jal ra, target", 0x4000_0001, 2),
        #[cfg(feature = "riscv")]
        (Arch::Rv64, "beq a0, a1, target", 0x4000_0001, 2),
    ];
    for &(arch, src, target, alignment) in cases {
        let mut asm = Assembler::new(arch);
        asm.base_address(0x4000_0000);
        asm.define_external("target", target);
        asm.emit(src).unwrap();
        match asm.finish() {
            Err(AsmError::MisalignedBranchTarget { alignment: a, .. }) => {
                assert_eq!(a, alignment, "{arch:?} `{src}`");
            }
            other => panic!("{arch:?} `{src}` to {target:#x} should be rejected, got {other:?}"),
        }
    }

    // Correctly aligned targets still assemble.
    #[cfg(feature = "aarch64")]
    {
        let mut asm = Assembler::new(Arch::Aarch64);
        asm.base_address(0x4000_0000);
        asm.define_external("target", 0x4000_0100);
        asm.emit("b target").unwrap();
        assert_eq!(asm.finish().unwrap().len(), 4);
    }
}

/// Regression: a label address is caller-supplied and may sit anywhere in the
/// 64-bit space, so the branch displacement can exceed `i64`. Computing it with
/// plain arithmetic panicked on overflow instead of reporting the target as
/// unreachable.
#[test]
#[cfg(feature = "arm")]
fn extreme_label_address_does_not_overflow() {
    let mut asm = Assembler::new(Arch::Thumb);
    asm.base_address(0x4000_0000);
    asm.define_external("target", 0x7FFF_FFFF_D800_FFC0);
    asm.emit("b target").unwrap();
    // Must be a diagnostic, not a panic — and certainly not silent success.
    assert!(asm.finish().is_err());
}

/// Regression: operand counts come from parsed text, so filling the
/// fixed-capacity operand list has to be fallible. An instruction naming more
/// operands than any encoding takes used to panic, which for a library
/// assembling untrusted input is a denial of service.
#[test]
#[cfg(feature = "x86_64")]
fn too_many_operands_is_an_error_not_a_panic() {
    let err = assemble("add rax, rbx, rcx, rdx, r8, r9, r10", Arch::X86_64).unwrap_err();
    assert!(matches!(err, AsmError::InvalidOperands { .. }), "{err}");
    // The legitimate wide forms still assemble.
    assert!(assemble("vpternlogd zmm0, zmm1, zmm2, 5", Arch::X86_64).is_ok());
}

/// Regression: an encoder that indexes further than the instruction actually
/// goes must produce a diagnostic, not panic. `la` with a missing operand was
/// one such case; indexing an `OperandList` past its length now yields
/// `Operand::Missing`, which every extraction helper rejects.
#[test]
#[cfg(feature = "riscv")]
fn missing_operands_are_diagnosed_not_panicked() {
    for src in ["la", "la a0", "jal", "beq a0"] {
        assert!(
            assemble(src, Arch::Rv64).is_err(),
            "`{src}` should be rejected"
        );
    }
}

/// Regression: exceeding the macro recursion limit must return an error, not
/// overflow the native stack (which aborts the process and cannot be caught by
/// an embedding host).
#[test]
#[cfg(feature = "x86_64")]
fn macro_recursion_limit_errors_without_stack_overflow() {
    let mut src = String::from(".macro recurse\nrecurse\n.endm\n");
    src.push_str("recurse\n");
    let mut asm = Assembler::new(Arch::X86_64);
    assert!(matches!(
        asm.emit(&src),
        Err(AsmError::ResourceLimitExceeded { .. })
    ));
}

/// Regression: RIP-relative addressing must correctly compute displacement
/// from the end of the current instruction, not from the start.
#[test]
#[cfg(feature = "x86_64")]
fn rip_relative_displacement_from_instruction_end() {
    let mut asm = Assembler::new(Arch::X86_64);
    asm.emit("lea rax, [rip + 0]\nnop").unwrap();
    let result = asm.finish().unwrap();
    // LEA RAX, [RIP+0] = 48 8D 05 00 00 00 00 (7 bytes), NOP = 90
    assert_eq!(
        &result.bytes()[..7],
        &[0x48, 0x8D, 0x05, 0x00, 0x00, 0x00, 0x00]
    );
}

/// Regression: ARM32 NOP must encode as MOV R0, R0 (pre-ARMv6K encoding),
/// not the ARMv6K+ hint NOP.
#[test]
#[cfg(feature = "arm")]
fn arm32_nop_is_mov_r0_r0() {
    let code = assemble("nop", Arch::Arm).unwrap();
    assert_eq!(code, &[0x00, 0x00, 0xA0, 0xE1]);
}
