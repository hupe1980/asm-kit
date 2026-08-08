+++
title = "ISA Reference"
description = "Complete per-architecture instruction references for x86-64, AArch64, ARM32, Thumb and RISC-V, with operand forms and encoding notes."
weight = 5
sort_by = "weight"
template = "section.html"
page_template = "page.html"
+++

Complete instruction references for each supported architecture.

---

Each reference page lists every supported instruction with its syntax, operand forms,
and encoding notes. Click an architecture below to view its full reference.

| Architecture | Page | Scope |
|:-------------|:-----|:------|
| [x86-64](@/docs/reference/x86_64.md) | x86 / x86-64 | Data movement, ALU, SIMD (SSE–AVX-512), AES-NI, SHA, BMI, FMA, TSX |
| [AArch64](@/docs/reference/aarch64.md) | ARM64 / A64 | Data processing, shifted/extended operands, load/store, NEON/AdvSIMD, LSE atomics, SVE (opt-in) |
| [ARM32](@/docs/reference/arm32.md) | ARMv7 / A32 | Data processing, barrel shifter, condition codes, load/store |
| [Thumb](@/docs/reference/thumb.md) | Thumb / T32 | 16-bit Thumb-1, 32-bit Thumb-2, IT blocks |
| [RISC-V](@/docs/reference/riscv.md) | RV32I / RV64I | Base ISA, M/A/F/D/V/C extensions |

## Notation Conventions

All reference pages use a consistent notation:

| Symbol | Meaning |
|:-------|:--------|
| `r64` / `r32` / `r16` / `r8` | Register of the specified bit width |
| `[mem]` | Memory operand |
| `imm` / `#imm` | Immediate value |
| `label` | Label reference (resolved by the linker) |
| `cond` | Condition code |
