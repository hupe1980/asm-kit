#![cfg(all(not(target_arch = "wasm32"), feature = "std"))]
//! Assemble every instruction snippet in `site/content/docs/reference/*.md`.
//!
//! The reference pages are the project's statement of what it supports, so a
//! snippet the assembler rejects is a defect in one or the other. Checking
//! them mechanically is how the UAL `#immediate`, barrel-shift, register-range
//! and `LDUR`/`STUR` gaps were found: each was documented in detail, and none
//! of them assembled.
//!
//! Snippets written in placeholder metasyntax (`add rd, rs1, rs2`) are not
//! assembly and are skipped. Everything else must assemble, so a newly
//! documented instruction fails this test until it is implemented — which is
//! the point.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use asm_rs::{Arch, Assembler, Syntax};

/// Operand placeholders used in the reference tables. A line containing one of
/// these as a whole word is notation, not an instruction.
const PLACEHOLDERS: &[&str] = &[
    "rd",
    "rs",
    "rs1",
    "rs2",
    "rn",
    "rm",
    "rt",
    "rt2",
    "ra",
    "imm",
    "imm3",
    "imm4",
    "imm5",
    "imm6",
    "imm8",
    "imm12",
    "imm16",
    "imm32",
    "imm64",
    "immr",
    "imms",
    "offset",
    "mem",
    "label",
    "r8",
    "r16",
    "r32",
    "r64",
    "xn",
    "xm",
    "xd",
    "xt",
    "wn",
    "wm",
    "wd",
    "wt",
    "vn",
    "vd",
    "vm",
    "zn",
    "zd",
    "zm",
    "pg",
    "shift",
    "cond",
    "csr",
    "uimm",
    "sym",
    "target",
    "func",
    "addr",
    "reg",
    "vs1",
    "vs2",
    "xs1",
    "count",
    "amount",
    "width",
    "lsb",
    "xmm",
    "ymm",
    "zmm",
    "mm",
    "k1",
    "cc",
    "n",
    "m",
    "sp_offset",
    "far_label",
    "loop_start",
    "value",
];

fn contains_placeholder(operands: &str) -> bool {
    operands
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .any(|w| PLACEHOLDERS.contains(&w.to_ascii_lowercase().as_str()))
}

/// Some table rows list alternatives rather than one instruction
/// (`stosw / stosd / stosq`). That is notation too.
fn is_alternation(line: &str) -> bool {
    line.contains(" / ")
}

/// AT&T snippets are recognisable by the `%` register sigil.
fn syntax_for(snippet: &str, arch: Arch) -> Syntax {
    if snippet.contains('%') {
        Syntax::Att
    } else if matches!(arch, Arch::Arm | Arch::Thumb | Arch::Aarch64) {
        Syntax::Ual
    } else {
        Syntax::Intel
    }
}

/// The comment character depends on the dialect, so strip accordingly.
fn strip_comment(line: &str, arch: Arch) -> &str {
    let line = line.split("//").next().unwrap_or("");
    let line = match arch {
        Arch::Arm | Arch::Thumb | Arch::Aarch64 => line.split('@').next().unwrap_or(""),
        _ => line.split('#').next().unwrap_or(""),
    };
    line.trim()
}

fn arch_for(path: &Path) -> Option<Arch> {
    let name = path.file_stem()?.to_str()?;
    Some(match name {
        "aarch64" => Arch::Aarch64,
        "arm32" => Arch::Arm,
        "thumb" => Arch::Thumb,
        "riscv" => Arch::Rv64,
        "x86_64" => Arch::X86_64,
        _ => return None,
    })
}

fn docs_reference_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR is crates/asm-rs; the site lives at the repo root.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("site/content/docs/reference")
}

/// Collect `(snippet, arch)` pairs from every ```asm block in a page.
fn snippets(text: &str, arch: Arch) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_asm = false;
    for line in text.lines() {
        if line.starts_with("+++") {
            continue;
        }
        if line.starts_with("```") {
            in_asm = line.starts_with("```asm");
            continue;
        }
        if !in_asm {
            continue;
        }
        let stripped = strip_comment(line, arch);
        if stripped.is_empty() || stripped.starts_with('.') || is_alternation(stripped) {
            continue;
        }
        // Split mnemonic from operands so a mnemonic like `mov` is not
        // mistaken for the `m` placeholder.
        let operands = stripped
            .split_once(char::is_whitespace)
            .map_or("", |(_, rest)| rest);
        if contains_placeholder(operands) {
            continue;
        }
        out.push(stripped.to_string());
    }
    out
}

/// Assemble one snippet the way a user would.
///
/// A full `Assembler` run rather than `encode_one`, because some documented
/// forms — literal pools (`ldr x0, =0xDEADBEEF`) above all — only work with
/// the assembler's state behind them. Label operands are satisfied by
/// predefining the names the reference pages use.
fn try_assemble(snippet: &str, arch: Arch) -> Result<(), asm_rs::AsmError> {
    let mut asm = Assembler::new(arch);
    asm.syntax(syntax_for(snippet, arch));
    for name in [
        "label",
        "target",
        "loop",
        "done",
        "far_label",
        "msg",
        "data",
        "func",
        "start",
        "end",
    ] {
        asm.define_external(name, 0x1000);
    }
    asm.emit(snippet)?;
    asm.finish().map(|_| ())
}

#[test]
fn every_documented_instruction_assembles() {
    let dir = docs_reference_dir();
    assert!(
        dir.is_dir(),
        "reference pages not found at {} — this test must be run from the workspace",
        dir.display()
    );

    let mut failures: Vec<String> = Vec::new();
    let mut checked = 0usize;
    let mut pages = 0usize;

    // Sorted so failure output is stable across runs.
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("read docs/reference")
        .map(|e| e.expect("dir entry").path())
        .collect();
    entries.sort();

    for path in entries {
        let Some(arch) = arch_for(&path) else {
            continue;
        };
        pages += 1;
        let text = std::fs::read_to_string(&path).expect("read reference page");
        let page = path.file_name().unwrap().to_string_lossy().to_string();

        // Deduplicate: several pages list the same instruction in more than
        // one table, and one failure per distinct snippet is enough.
        let mut seen = BTreeSet::new();
        for snippet in snippets(&text, arch) {
            if !seen.insert(snippet.clone()) {
                continue;
            }
            checked += 1;
            if let Err(e) = try_assemble(&snippet, arch) {
                failures.push(format!("  {page}: `{snippet}`\n      {e}"));
            }
        }
    }

    assert!(pages > 0, "no reference pages matched a known architecture");
    assert!(
        failures.is_empty(),
        "{} of {checked} documented instructions do not assemble:\n{}\n\
         \nEither implement them or correct the documentation — the reference \
         pages are the project's statement of what it supports.",
        failures.len(),
        failures.join("\n")
    );
}
