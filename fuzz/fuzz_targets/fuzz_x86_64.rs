#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &str| {
    // Fuzz the one-shot assembler — must never panic, only return Ok/Err.
    let _ = asm_kit::assemble(data, asm_kit::Arch::X86_64);

    // Also fuzz with a non-zero base address.
    let _ = asm_kit::assemble_at(data, asm_kit::Arch::X86_64, 0x400000);

    // Fuzz the builder API with multiple emit calls (split on newlines).
    let mut asm = asm_kit::Assembler::new(asm_kit::Arch::X86_64);
    for line in data.lines() {
        if asm.emit(line).is_err() {
            return;
        }
    }
    let _ = asm.finish();
});
