# asm-rs

A pure Rust multi-architecture assembly engine for offensive security.

[![Crates.io](https://img.shields.io/crates/v/asm-rs.svg)](https://crates.io/crates/asm-rs)
[![docs.rs](https://docs.rs/asm-rs/badge.svg)](https://docs.rs/asm-rs)
[![CI](https://github.com/hupe1980/asm-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/hupe1980/asm-rs/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)

[📖 Documentation](https://hupe1980.github.io/asm-rs/) · [📚 API Reference](https://docs.rs/asm-rs) · [📦 Crate](https://crates.io/crates/asm-rs)

> Zero `unsafe`, `no_std`-compatible, designed for embedding in exploit compilers, JIT engines, security tools, and shellcode generators.

## ✨ Features

- 🦀 **Pure Rust** — `#![forbid(unsafe_code)]`, no C dependencies
- 📦 **`no_std` support** — embedded and WASM environments (with `alloc`)
- 🏗️ **Multi-architecture** — x86, x86-64, ARM32, Thumb/Thumb-2, AArch64, RISC-V
- 🔄 **Intel, AT&T & UAL syntax** — GAS-compatible AT&T via `.syntax att`, plus UAL `#immediate` and `@`/`//` comments for ARM/Thumb/AArch64
- 🏷️ **Labels & constants** — forward/backward references, numeric labels, `.equ`/`.set`
- 🔀 **Branch relaxation** — Szymanski's algorithm for optimal branch encoding
- 📐 **Preprocessor** — `.macro`/`.rept`/`.irp`/`.if`/`.ifdef` directives
- ⚡ **Peephole optimizer** — MOV narrowing and REX elimination by default, all semantics-preserving; FLAGS-clobbering rewrites are opt-in
- 🛡️ **Bounded by construction** — configurable limits on statements, labels, output size, recursion and macro expansion for untrusted input
- 🔧 **Compile-time macros** — `asm_bytes!`/`asm_array!` for zero-overhead assembly
- 🎯 **Literal pools** — `LDR Xn/Rn, =value` with automatic pool management (AArch64/ARM32)
- 📋 **Listing output** — human-readable address/hex/source listing for debugging
- 🔗 **Applied relocations** — full relocation info exposed for tooling
- 🧬 **Serde support** — optional serialization for all public types

## 🏛️ Supported Architectures

| Architecture | Variants | Highlights |
|:---|:---|:---|
| **x86** | 32-bit, 16-bit real mode | Full ISA, `.code16`/`.code32` |
| **x86-64** | 64-bit | SSE–SSE4.2, AVX/AVX2, AVX-512, AES-NI, BMI1/2, FMA3 |
| **ARM32** | A32 (ARMv7) | Condition codes, barrel shifter, register ranges, literal pools |
| **Thumb** | T16/T32 | Auto 16/32-bit encoding, IT blocks, register ranges |
| **AArch64** | A64 (ARMv8+) | NEON/AdvSIMD, LSE atomics, shifted/extended/unscaled operands, vector lanes, literal pools |
| **RISC-V** | RV32I, RV64I | M/A/C extensions, auto-compression |

## 🚀 Quick Start

```sh
cargo add asm-rs
```

### One-Shot Assembly

```rust
use asm_rs::{assemble, Arch};

let bytes = assemble("mov eax, 42\nret", Arch::X86_64).unwrap();
assert_eq!(bytes[0], 0xB8); // mov eax, imm32
```

### Builder API

```rust
use asm_rs::{Assembler, Arch};

let mut asm = Assembler::new(Arch::X86_64);
asm.emit("push rbp").unwrap();
asm.emit("mov rbp, rsp").unwrap();
asm.emit("sub rsp, 0x20").unwrap();
// ... function body ...
asm.emit("add rsp, 0x20").unwrap();
asm.emit("pop rbp").unwrap();
asm.emit("ret").unwrap();

let result = asm.finish().unwrap();
println!("Generated {} bytes", result.len());
```

### AT&T / GAS Syntax

```rust
use asm_rs::{Assembler, Arch, Syntax};

let mut asm = Assembler::new(Arch::X86_64);
asm.syntax(Syntax::Att);
asm.emit(r#"
    pushq %rbp
    movq %rsp, %rbp
    movl $42, %eax
    popq %rbp
    ret
"#).unwrap();
```

### ARM / AArch64 with UAL Syntax

```rust
use asm_rs::{assemble, Arch};

// `#` is the immediate prefix for ARM/Thumb/AArch64 (as in GNU as and the
// Arm reference manuals); `@` and `//` start comments.
let bytes = assemble("mov x0, #1  @ exit code", Arch::Aarch64).unwrap();

// The prefix is optional — these assemble identically.
assert_eq!(bytes, assemble("mov x0, 1", Arch::Aarch64).unwrap());

// Barrel shifts and register extends are trailing operands, register lists
// accept ranges, and vector lanes are addressable.
assemble("add x0, x1, x2, lsl #3", Arch::Aarch64).unwrap();
assemble("add x0, x1, w2, uxtw #2", Arch::Aarch64).unwrap();
assemble("umov w0, v0.s[2]", Arch::Aarch64).unwrap();
assemble("add r0, r1, r2, lsl r3", Arch::Arm).unwrap();
assemble("push {r0-r7}", Arch::Thumb).unwrap();
```

### Multi-Architecture Shellcode

```rust
use asm_rs::{Assembler, Arch};

// x86-64
let bytes = asm_rs::assemble("xor edi, edi; mov eax, 60; syscall", Arch::X86_64).unwrap();

// AArch64
let mut asm = Assembler::new(Arch::Aarch64);
asm.emit("mov x0, #0; mov x8, #93; svc #0").unwrap();

// ARM32
let mut asm = Assembler::new(Arch::Arm);
asm.emit("mov r0, #0; mov r7, #1; svc #0").unwrap();

// RISC-V
let bytes = asm_rs::assemble("li a7, 93; li a0, 0; ecall", Arch::Rv32).unwrap();
```

### Compile-Time Assembly

```rust
use asm_rs_macros::{asm_bytes, asm_array};

const SHELLCODE: &[u8] = asm_bytes!(x86_64, "xor eax, eax; inc eax; ret");
const NOP: [u8; 1] = asm_array!(x86_64, "nop");
const ARM_CODE: &[u8] = asm_bytes!(arm, "bx lr");
```

> See [`crates/asm-rs-macros/README.md`](crates/asm-rs-macros/README.md) for full proc-macro documentation.

## 🧪 Testing

Extensive test suite covering unit, integration, property-based (proptest), and fuzz testing (`cargo-fuzz`), with zero warnings.

Machine code is **cross-validated against independent decoders** rather than
against itself — iced-x86 for x86/x86-64, bad64 and yaxpeax-arm for
AArch64/ARM/Thumb, riscv-decode for RISC-V — at two levels:

- **Encoding** — each instruction is assembled standalone and decoded back.
- **Relocation** — branches to labels are swept across their full displacement
  range, including the sign boundary and the ends of each encoding, and the
  address the reference decoder computes is compared to the intended target.
  Relocation bugs are invisible near zero displacement, so only a sweep finds
  them.
- **Differential fuzzing** — a fuzz target generates a branch to a known
  address and asks an independent decoder where it actually goes. Asserting
  "does not panic" cannot catch machine code that is well-formed but means the
  wrong thing; this can.
- **Documentation** — every instruction in the reference pages is assembled by
  a test, so the docs cannot drift away from what the library supports.

## ⚙️ Configuration

### Cargo Features

| Feature | Default | Description |
|:---|:---:|:---|
| `std` | ✅ | Standard library support |
| `x86` | ✅ | x86 (32-bit) backend |
| `x86_64` | ✅ | x86-64 backend |
| `arm` | ✅ | ARM32 + Thumb/Thumb-2 backend |
| `aarch64` | ✅ | AArch64 backend |
| `riscv` | ✅ | RISC-V backend |
| `avx` | ✅ | AVX/AVX2/FMA |
| `avx512` | ✅ | AVX-512/EVEX |
| `neon` | ✅ | AArch64 NEON/AdvSIMD |
| `sve` | ❌ | AArch64 SVE |
| `riscv_f` | ✅ | RISC-V F/D floating-point |
| `riscv_v` | ❌ | RISC-V V vector |
| `serde` | ❌ | Serialize/Deserialize for public types |

### MSRV

Rust **1.75** or later.

## 📖 Learn More

For the full reference — architecture details, ISA instruction tables, directives, API docs, and configuration options — visit the **[documentation site](https://hupe1980.github.io/asm-rs/)**.

## 📄 License

Licensed under either of:

- [Apache License, Version 2.0](LICENSE-APACHE)
- [MIT License](LICENSE-MIT)

at your option.
