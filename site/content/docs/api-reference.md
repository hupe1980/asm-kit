+++
title = "API Overview"
description = "How the asm-rs API is shaped: one-shot functions, the Assembler builder, AssemblyResult, error handling and the compile-time macros."
weight = 3
+++

This page explains how the API fits together and which entry point to reach for.
For exact signatures, field types and every method — always current, because it
is generated from the source — see
[docs.rs/asm-rs](https://docs.rs/asm-rs).

## Three ways in

| You want | Use | Cost |
|:---------|:----|:-----|
| Bytes from a string, nothing else | [`assemble`](https://docs.rs/asm-rs/latest/asm_rs/fn.assemble.html) | One call |
| Labels, a base address, listings, limits | [`Assembler`](https://docs.rs/asm-rs/latest/asm_rs/assembler/struct.Assembler.html) | A few lines of setup |
| A constant known at compile time | [`asm_bytes!`](https://docs.rs/asm-rs-macros/) | Zero at runtime |

### One-shot

```rust
use asm_rs::{assemble, assemble_at, Arch};

// Position-independent: labels resolve relative to 0.
let code = assemble("
    xor edi, edi
    mov eax, 60
    syscall
", Arch::X86_64)?;

// When the code will live at a known address, say so — absolute
// relocations and PC-relative displacements both depend on it.
let at_addr = assemble_at("nop\nret", Arch::X86_64, 0x40_1000)?;
```

### Builder

`Assembler` is the full pipeline. `emit()` may be called repeatedly — each call
appends — and `finish()` consumes the assembler to resolve labels, relax
branches and produce the output.

```rust
use asm_rs::{Assembler, Arch};

let mut asm = Assembler::new(Arch::X86_64);
asm.base_address(0x40_1000);
asm.enable_listing();
asm.define_constant("EXIT_CODE", 0);

asm.emit("
_start:
    mov edi, EXIT_CODE
    mov eax, 60
    syscall
")?;

let result = asm.finish()?;
println!("{}", result.listing());
```

Configuration is set before emitting: `base_address`, `syntax`, `optimize`,
`limits`, `enable_listing`, `define_constant`, `define_external` and
`define_preprocessor_symbol`. There are also builder methods for emitting data
directly — `db`, `dw`, `dd`, `dq`, `ascii`, `asciz`, `align`, `org`, `fill`,
`space` — for when it is easier to call a method than to format a directive.

`reset()` clears the assembled state while keeping configuration, so one
assembler can be reused across independent jobs.

## What comes back

[`AssemblyResult`](https://docs.rs/asm-rs/latest/asm_rs/assembler/struct.AssemblyResult.html)
holds the bytes plus everything the linker learned while producing them:

```rust
use asm_rs::{Assembler, Arch};

let mut asm = Assembler::new(Arch::X86_64);
asm.emit("start:\n  jmp start")?;
let result = asm.finish()?;

let code: &[u8] = result.bytes();
let entry: Option<u64> = result.label_address("start");

// Labels are sorted by name, so `label_address` is a binary search.
for (name, addr) in result.labels() {
    println!("{name} = {addr:#x}");
}

// Where each label reference was patched — enough to re-link elsewhere.
for reloc in result.relocations() {
    println!("{} at +{:#x} ({:?})", reloc.label, reloc.offset, reloc.kind);
}
```

`into_bytes()` takes ownership when the metadata is not needed.

## Errors

Every fallible call returns
[`AsmError`](https://docs.rs/asm-rs/latest/asm_rs/error/enum.AsmError.html),
which carries a [`Span`](https://docs.rs/asm-rs/latest/asm_rs/error/struct.Span.html)
— line, column, byte offset and length — so a diagnostic can point at the
source. It implements `Display`, and `std::error::Error` when the `std` feature
is on.

The variants distinguish causes worth handling differently: an unknown
mnemonic, invalid operands, an out-of-range immediate, an undefined or
duplicate label, a branch that is out of range or misaligned for its encoding,
a relaxation that did not converge, and an exceeded resource limit. `Multiple`
carries several at once when a single `emit()` produced more than one.

```rust
use asm_rs::{assemble, Arch, AsmError};

match assemble("mov rax, rbx, rcx", Arch::X86_64) {
    Ok(bytes) => println!("{} bytes", bytes.len()),
    Err(AsmError::InvalidOperands { detail, span }) => {
        eprintln!("line {}: {detail}", span.line);
    }
    Err(e) => eprintln!("{e}"),
}
```

## Compile-time macros

`asm-rs-macros` runs the assembler during compilation and emits a constant, so
there is no runtime cost — and no runtime failure mode, because a bad
instruction is a compile error.

```rust
use asm_rs_macros::{asm_array, asm_bytes};

const SHELLCODE: &[u8] = asm_bytes!(x86_64, "xor eax, eax; inc eax; ret");
const NOP: [u8; 1] = asm_array!(x86_64, "nop");

// An optional base address may precede the source.
const AT_ADDR: &[u8] = asm_bytes!(x86_64, 0x400000, "nop; ret");
```

The architecture token is one of `x86`, `x86_64`, `arm`, `thumb`, `aarch64`,
`rv32` or `rv64`. Use `asm_array!` when the length must be known in the type —
it produces `[u8; N]` rather than a slice.

## Feature gates

Each architecture backend is a Cargo feature, so a build only pays for the
targets it uses. See [Configuration](@/docs/configuration.md#cargo-features)
for the full list and the defaults.
