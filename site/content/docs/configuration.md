+++
title = "Configuration"
description = "Cargo features, optimization levels, syntax dialects, directives, resource limits, shifted operands and branch alignment rules."
weight = 2
+++

## Cargo Features

### Architecture Backends

| Feature | Default | Description |
|:--------|:--------|:------------|
| `x86` | Yes | Enable x86 (32-bit) backend |
| `x86_64` | Yes | Enable x86-64 backend |
| `arm` | Yes | Enable ARM32 (A32) + Thumb/Thumb-2 (T32) backend |
| `aarch64` | Yes | Enable AArch64 (A64) backend |
| `riscv` | Yes | Enable RISC-V (RV32I/RV64I + M + A + C extensions) backend |

### SIMD / Extension Features

| Feature | Default | Description |
|:--------|:--------|:------------|
| `avx` | Yes | AVX/AVX2/FMA instructions (requires `x86_64`) |
| `avx512` | Yes | AVX-512/EVEX instructions with opmask + broadcast (requires `x86_64`) |
| `neon` | Yes | AArch64 Advanced SIMD (NEON) instructions (requires `aarch64`) |
| `sve` | No | AArch64 Scalable Vector Extension (requires `aarch64`) |
| `riscv_f` | Yes | RISC-V F/D floating-point extensions (requires `riscv`) |
| `riscv_v` | No | RISC-V V vector extension (requires `riscv`) |

### Other Features

| Feature | Default | Description |
|:--------|:--------|:------------|
| `std` | Yes | Enable standard library support (`std::error::Error`) |
| `serde` | No | Enable `Serialize`/`Deserialize` derives for all public types |

### Usage Examples

```sh
# Default: all architectures, all default SIMD extensions
cargo add asm-kit

# x86-64 only (minimal binary size)
cargo add asm-kit --no-default-features --features std,x86_64,avx,avx512

# no_std for embedded/WASM
cargo add asm-kit --no-default-features --features x86_64

# With serde support
cargo add asm-kit --features serde

# ARM only
cargo add asm-kit --no-default-features --features std,arm,aarch64,neon
```

## Runtime Options

### Optimization Level

Peephole optimizations are controlled at runtime via `OptLevel`:

```rust
use asm_kit::{Assembler, Arch, OptLevel};

let mut asm = Assembler::new(Arch::X86_64);
asm.optimize(OptLevel::Size);        // Shorter encodings, same semantics (default)
asm.optimize(OptLevel::None);        // Emit exactly what was written
asm.optimize(OptLevel::Aggressive);  // Also allow FLAGS-clobbering rewrites
```

The levels differ in *what an optimization may change*, not just how hard it
tries:

| Level | May shrink encodings | May change architectural state |
|:------|:--------------------:|:-------------------------------|
| `None` | no | no |
| `Size` (default) | yes | no |
| `Aggressive` | yes | yes — FLAGS only |

`OptLevel::Size` is the default because every transform it performs is
**observationally equivalent**: the rewritten instruction leaves registers,
memory *and FLAGS* exactly as the original would, so you never have to reason
about whether optimization was enabled.

| Pattern | Replacement | Savings | Level |
|:--------|:------------|:--------|:------|
| `mov reg64, small_imm` | `mov reg32, imm32` | 7 → 5 bytes | `Size` |
| `and reg64, u32_imm` | `and reg32, u32_imm` | 1 byte (REX removed) | `Size` |
| `and reg, reg` | `test reg, reg` | Same size, avoids the register write | `Size` |
| `mov reg64, 0` | `xor reg32, reg32` | 5–7 → 2 bytes | `Aggressive` |

The zero idiom is gated behind `Aggressive` because `xor` writes FLAGS while
`mov` does not. Applying it by default would silently break sequences like:

```asm
cmp  eax, ebx
mov  eax, 0     ; must not disturb FLAGS
sete al
```

Enable `Aggressive` only where FLAGS are dead after the rewritten instruction.

### Syntax

```rust
use asm_kit::{Assembler, Arch, Syntax};

let mut asm = Assembler::new(Arch::X86_64);
asm.syntax(Syntax::Att);   // AT&T / GAS syntax
asm.syntax(Syntax::Intel); // Intel syntax (default for x86)
```

You can also switch syntax mid-stream:

```asm
.syntax att
movq $42, %rax
.syntax intel
mov rbx, 42
```

### Base Address

```rust
use asm_kit::{Assembler, Arch};

let mut asm = Assembler::new(Arch::X86_64);
asm.base_address(0x401000); // Set the base address for label resolution
```

### External Labels

Pre-define label addresses for linking against external code:

```rust
use asm_kit::{Assembler, Arch};

let mut asm = Assembler::new(Arch::X86_64);
asm.define_external("printf", 0x401000);
asm.emit("mov rax, printf").unwrap();
```

### Constants

Define assembly-time constants:

```rust
use asm_kit::{Assembler, Arch};

let mut asm = Assembler::new(Arch::X86_64);
asm.define_constant("SYS_EXIT", 60);
asm.define_constant("BUFFER_SIZE", 4096);
asm.emit("mov eax, SYS_EXIT").unwrap();
```

### Preprocessor Symbols

Define symbols for conditional assembly:

```rust
use asm_kit::{Assembler, Arch};

let mut asm = Assembler::new(Arch::X86_64);
asm.define_preprocessor_symbol("DEBUG", 1);
asm.define_preprocessor_symbol("VERSION", 2);
asm.emit(r#"
.ifdef DEBUG
    int 3
.endif
"#).unwrap();
```

### Listing Output

Enable human-readable listing output for debugging:

```rust
use asm_kit::{Assembler, Arch};

let mut asm = Assembler::new(Arch::X86_64);
asm.base_address(0x401000);
asm.enable_listing();
asm.emit("entry:\npush rbp\nmov rbp, rsp\nret").unwrap();
let result = asm.finish().unwrap();

println!("{}", result.listing());
// 00401000                  entry:
// 00401000  55                push rbp
// 00401001  4889E5            mov rbp, rsp
// 00401004  C3                ret
```

## Resource Limits

`asm-kit` is built to assemble text it did not write — exploit compilers, JIT
front-ends and shellcode generators routinely feed it attacker-influenced
input. Every unbounded operation in the pipeline is therefore metered, and
exceeding a limit returns `AsmError::ResourceLimitExceeded` rather than
exhausting memory or aborting the process.

```rust
use asm_kit::{Assembler, Arch, ResourceLimits};

let mut asm = Assembler::new(Arch::X86_64);
asm.limits(ResourceLimits {
    max_statements: 10_000,
    max_labels: 1_000,
    max_output_bytes: 64 * 1024,
    max_errors: 16,
    max_recursion_depth: 16,
    max_source_bytes: 1024 * 1024,
    max_iterations: 10_000,
    max_expanded_bytes: 4 * 1024 * 1024,
});
```

| Limit | Default | Guards against |
|:------|--------:|:---------------|
| `max_source_bytes` | 64 MiB | Multi-gigabyte input consuming memory during lexing |
| `max_statements` | 1,000,000 | Unbounded IR growth from a large program |
| `max_labels` | 100,000 | Unbounded symbol-table growth |
| `max_output_bytes` | 16 MiB | Oversized images — see the note below |
| `max_errors` | 64 | Error accumulation on pathologically broken input |
| `max_recursion_depth` | 32 | Stack exhaustion from nested macro expansion |
| `max_iterations` | 100,000 | Runaway `.rept` / `.irp` / `.irpc` loops |
| `max_expanded_bytes` | 64 MiB | Preprocessor output size, independent of loop count |

Two of these deserve explanation, because the obvious limit does not cover the
actual risk:

**`max_output_bytes` is enforced during layout, not after.** `.org` and
`.align` request padding whose size is unrelated to the length of the source
text: `.org 0xFFFFFFFFFFF` is a 20-byte line asking for a 16 TiB image. The
ceiling is therefore checked while computing fragment offsets, *before* any
output buffer is reserved.

**`max_recursion_depth` is deliberately shallow.** Each level of macro or
`.rept` nesting costs a native stack frame. A limit deep enough to overflow the
stack would abort the process — which an embedding host cannot catch — so the
default is set well inside what a small embedded stack can absorb. Raise it
only if you know your stack can take it.

**`max_iterations` does not bound expansion size.** A single `.rept 50000`
wrapped around an 8 KiB body costs 50,000 iterations — comfortably under the
limit — while emitting 400 MiB of text. Mutually-invoking macros grow
exponentially with depth for the same reason. `max_expanded_bytes` meters the
bytes the preprocessor produces, which is the quantity that actually matters.

## Directives Reference

| Directive | Description |
|:----------|:------------|
| `.byte` / `.db` | Emit raw bytes |
| `.word` / `.dw` / `.short` | Emit 16-bit values |
| `.long` / `.dd` / `.int` | Emit 32-bit values |
| `.quad` / `.dq` | Emit 64-bit values |
| `.ascii` | Emit string (no terminator) |
| `.asciz` / `.string` | Emit null-terminated string |
| `.equ` / `.set` | Define named constant |
| `name = value` | Alternative constant syntax |
| `.align` / `.balign` | Align to byte boundary |
| `.p2align` | Align to power-of-2 boundary |
| `.fill` | Fill with repeated pattern |
| `.space` / `.skip` | Reserve zero-filled space |
| `.org` | Set origin address (with optional fill byte) |
| `.global` / `.globl` | Declare global symbol (accepted, no-op) |
| `.section` | Declare section (accepted, no-op) |
| `.macro` / `.endm` | Define and end a macro |
| `.rept` / `.endr` | Repeat block N times |
| `.irp` / `.endr` | Iterate over value list |
| `.irpc` / `.endr` | Iterate over characters |
| `.if` / `.else` / `.elseif` / `.endif` | Conditional assembly |
| `.ifdef` / `.ifndef` | Test if symbol is defined |
| `.code16` / `.code32` / `.code64` | Switch operand/address size mode |
| `.syntax att` / `.syntax intel` | Switch assembly syntax |
| `.option rvc` / `.option norvc` | Enable/disable RISC-V C extension |
| `.ltorg` / `.pool` | Flush literal pool (AArch64/ARM) |
| `.thumb` / `.arm` | Switch ARM/Thumb mode |
| `.thumb_func` | Mark next label as Thumb function |

## Comments & Separators

The comment character depends on the syntax dialect, exactly as it does in
GNU `as` — because `#` is the immediate prefix in ARM/Thumb/AArch64 assembly
and cannot also introduce a comment there.

| Dialect | Targets | Comments | Immediate prefix |
|:--------|:--------|:---------|:-----------------|
| Intel / AT&T / RISC-V | x86, x86-64, RV32, RV64 | `#`, `//` | none (`$` in AT&T) |
| UAL | ARM, Thumb, AArch64 | `@`, `//` | `#` (optional) |

```asm
; --- x86-64 / RISC-V ---
# Hash comments
mov rax, rbx    # inline comment
mov rcx, rdx    // also works

; --- ARM / Thumb / AArch64 ---
@ At-sign comments
add r0, r1, r2  @ inline comment
add x0, x1, x2  // also works
mov x0, #1      @ `#` is the immediate prefix here, not a comment

; Semicolons are statement separators everywhere
nop; nop; ret
```

The `#` prefix on UAL immediates is optional — `mov x0, 1` and `mov x0, #1`
assemble identically — but writing it keeps sources copy-pasteable to and from
GNU `as` and the Arm reference manuals.

## Shifted & Extended Operands

ARM, Thumb and AArch64 write a barrel shift as a trailing operand on the
instruction:

```asm
@ --- ARM A32 ---
add   r0, r1, r2, lsl #3      @ r0 = r1 + (r2 << 3)
sub   r0, r1, r2, lsr #4      @ logical shift right
and   r0, r1, r2, asr #2      @ arithmetic shift right
orr   r0, r1, r2, ror #8      @ rotate right
add   r0, r1, r2, lsl r3      @ register-supplied amount
mov   r0, r1, rrx             @ rotate right through carry

@ A32 has no dedicated shift instructions — these are MOV aliases:
lsl   r0, r1, #4              @ == mov r0, r1, lsl #4
rrx   r0, r1                  @ == mov r0, r1, rrx

// --- AArch64 ---
add   x0, x1, x2, lsl #3      // shifted register (lsl/lsr/asr)
and   x0, x1, x2, ror #12     // logical ops also encode ror
add   x0, x1, #0x1, lsl #12   // shifted immediate (lsl #0 or #12 only)
movz  x0, #0x5678, lsl #16    // move-wide lane select (0/16/32/48)
ror   x0, x1, #4              // encoded as the EXTR alias
```

Combinations the architecture cannot encode are rejected rather than silently
dropped — an ignored shift would assemble cleanly and compute something else:

| Written | Rejected because |
|:--------|:-----------------|
| `add x0, x1, x2, ror #3` | ROR is reserved in the add/sub shifted-register form |
| `add w0, w1, w2, lsl #32` | amount exceeds the W-register width |
| `add x0, x1, #1, lsl #4` | add/sub immediates encode only `lsl #0` and `lsl #12` |
| `movz x0, #1, lsl #8` | move-wide shifts select a 16-bit lane |
| `add r0, r1, r2, lsl` | a shift needs an amount |

### Register Extends (AArch64)

An operand can be extended rather than shifted, which selects a different
encoding — and is the only form that can name `SP`:

```asm
add   x0, x1, w2, uxtw        // zero-extend the 32-bit operand
add   x0, x1, w2, uxtw #2     // ...and shift it left by 2
sub   x0, x1, w2, sxtw #3     // sign-extend
add   sp, sp, x0              // SP forces the extend encoding automatically
```

On AArch64, register number 31 means `SP` in the add/sub immediate, add/sub
extended-register and load/store base positions, and the zero register
everywhere else. `asm-kit` picks the encoding that can actually name `SP` when
one appears; `adds sp, ...` is rejected, because the flag-setting form cannot
write `SP` at all.

### Register Ranges

Register lists accept ranges as well as explicit enumeration:

```asm
push  {r0-r7}                 @ == push {r0, r1, r2, r3, r4, r5, r6, r7}
stmdb sp!, {r4-r11, lr}       @ ranges and singles may be mixed
```

## Branch Target Alignment

PC-relative branches store their displacement pre-scaled — AArch64 and ARM by
4, Thumb and RISC-V by 2 — so a target that is not aligned to that granularity
cannot be represented:

```asm
// AArch64, with `target` at an odd address
b     target          // error: branch target 'target' is not 4-byte aligned
```

Truncating the low bits would produce a perfectly valid instruction that
branches somewhere else, so this is an error
(`AsmError::MisalignedBranchTarget`) rather than a rounding.

## Minimum Supported Rust Version

**Rust 1.75** or later.

The MSRV is enforced in CI and verified on every push.
