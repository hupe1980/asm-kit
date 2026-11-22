#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &str| {
    // Fuzz both RV32 and RV64 targets.
    let _ = asm_kit::assemble(data, asm_kit::Arch::Rv32);
    let _ = asm_kit::assemble(data, asm_kit::Arch::Rv64);

    let _ = asm_kit::assemble_at(data, asm_kit::Arch::Rv64, 0x80000000);

    let mut asm = asm_kit::Assembler::new(asm_kit::Arch::Rv64);
    for line in data.lines() {
        if asm.emit(line).is_err() {
            return;
        }
    }
    let _ = asm.finish();
});
