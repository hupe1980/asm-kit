# Performance

Benchmark results for `asm-rs`, measured with [Criterion.rs](https://github.com/bheisler/criterion.rs).

**Hardware**: Apple Silicon (M-series), macOS, Rust 1.97.1.
**Run**: `cargo bench --bench throughput`

> Figures are the Criterion median. They move with the toolchain — the same
> suite on Rust 1.92 was roughly 2× slower on the realistic workloads — so the
> compiler version above is part of the measurement, not incidental.
>
> The **Δ** column compares against the pre-optimization tree, measured
> back-to-back on the same machine via a `git worktree` rather than against
> stored results, so the two builds see the same thermal and load conditions.

## Single Instruction Latency

| Benchmark | Time | Notes |
|---|---|---|
| `x86_64_nop` | ~257 ns | Simplest instruction |
| `x86_64_mov_reg_imm` | ~331 ns | REX + ModR/M + imm |
| `x86_64_add_reg_reg` | ~328 ns | REX + ModR/M |
| `x86_64_mov_mem` | ~700 ns | SIB + displacement |
| `x86_64_vaddps_avx` | ~428 ns | VEX 3-operand |
| `x86_64_vaddps_avx512` | ~430 ns | EVEX 3-operand |
| `aarch64_add` | ~370 ns | Fixed-width encoding |
| `aarch64_ldr` | ~417 ns | Load/store with offset |
| `arm_add` | ~363 ns | ARM A32 |
| `riscv64_add` | ~362 ns | R-type |
| `riscv64_lw` | ~374 ns | I-type load |

**Target**: < 1 µs for simple instructions — **achieved** for all cases.

## Throughput (Multi-Instruction)

| Benchmark | Time | Δ | Throughput |
|---|---|---|---|
| `x86_64_100_insn` | ~18.7 µs | −11% | ~64 MiB/s |
| `x86_64_1000_insn` | ~182 µs | −10% | ~67 MiB/s |
| `x86_64_5000_insn` | ~921 µs | −10% | ~66 MiB/s |
| `aarch64_1000_insn` | ~193 µs | −4% | ~65 MiB/s |
| `riscv_1000_insn` | ~166 µs | −11% | ~87 MiB/s |

Throughput is consistent at ~50–64 MiB/s across instruction counts and architectures, demonstrating linear scaling.

## Label Resolution

| Benchmark | Time | Δ |
|---|---|---|
| `50_labels` | ~34 µs | ~0% |
| `200_labels` | ~79 µs | −3% |
| `500_labels` | ~168 µs | −10% |

Near-linear scaling — no quadratic blowup in label resolution.

## Branch Relaxation

| Benchmark | Time | Δ | Notes |
|---|---|---|---|
| `short_branch_10_nop` | ~2.5 µs | −17% | No relaxation needed |
| `edge_branch_120_nop` | ~14 µs | −18% | Near boundary |
| `long_branch_200_nop` | ~24 µs | −13% | Relaxation triggered |

Relaxation adds minimal overhead — proportional to code size, not branch count.

## Preprocessor

| Benchmark | Time | Δ |
|---|---|---|
| `rept_100_nop` | ~15 µs | −10% |
| `macro_push_pair_7x` | ~6.0 µs | −10% |
| `conditional_assembly` | ~1.6 µs | −23% |

Source with no preprocessor directives skips expansion entirely. That check
runs over every line of every `emit()`, so it leads with a single byte
comparison — every directive it looks for begins with `.`, and almost no line
of real assembly does — before falling back to full prefix matching. On
directive-free input the scan costs **~330 ns per 1000 instructions**, down
from ~13.9 µs.

## API Comparison

| Benchmark | Time | Δ |
|---|---|---|
| `oneshot_4_insn` | ~843 ns | −23% |
| `builder_4_insn` | ~901 ns | −33% |
| `builder_4_insn_at_base` | ~900 ns | −17% |

The `assemble()` one-shot API is slightly faster than the builder for small sequences due to reduced overhead.

## Realistic Workloads

| Benchmark | Time | Δ |
|---|---|---|
| `syscall_stub` (5 insn) | ~1.09 µs | −16% |
| `function_prolog_epilog` (9 insn) | ~1.98 µs | −15% |
| `avx512_vector_body` (6 insn) | ~1.81 µs | −7% |
| `aarch64_function` (7 insn) | ~1.59 µs | −14% |
| `riscv_function` (10 insn) | ~1.92 µs | −13% |

Short sequences gain the most: the preprocessor scan was a fixed per-`emit()`
cost, so removing it helps most where there is least other work.

## Memory

Peak memory is proportional to the **output**, not the source. Statements are
consumed as they are parsed rather than collected into a `Vec` first:
`Instruction` stores its operand list inline (272 bytes, so that encoding needs
no heap allocation), which would otherwise cost ~272 MB at the default
1,000,000-statement limit for a program whose output is capped at 16 MiB.

This is a memory property, not a speed one — measured end-to-end, streaming is
within noise of buffering, because the allocator handles the `Vec` growth well.
It matters for embedding the assembler somewhere with a bounded heap.

| Type | Size | Note |
|---|---:|---|
| `Instruction` | 272 B | `OperandList` inline (208 B) + mnemonic + span |
| `OperandList` | 208 B | 6 × `Operand`; five- and six-operand forms are real (AVX-512, SVE) |
| `Operand` | 32 B | Width set by `Immediate(i128)`, which keeps `u64` and `i64` ranges distinguishable for diagnostics |
| `InstrBytes` | 33 B | Inline instruction buffer — no heap on the encoding path |

## Running Benchmarks

```bash
# Full benchmark suite
cargo bench --bench throughput

# Quick validation run
cargo bench --bench throughput -- --quick

# Specific group
cargo bench --bench throughput -- single_instruction
```

## Tracking Regressions

Criterion automatically compares against previous runs and reports statistically significant changes. Results are stored in `target/criterion/`.
