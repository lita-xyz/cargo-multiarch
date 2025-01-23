# cargo-multiarch

This cargo extension allows compiling multiple specialized versions of an executable into a single fat binary. At runtime the most optimal one is selected depending of the CPU features supported by your CPU.

## Installation

```bash
git clone https://github.com/lita-xyz/cargo-multiarch
cd cargo-multiarch
cargo install --locked --path .
```

## Usage

### From CLI, no project config
Navigate to your project root and for example run:

```
cargo multiarch --cpus x86-64-v1,x86-64-v3
```

This is equivalent to `cargo build --release` but will build a fat binary for base x86-64 and x86-64-v3 (i.e. AVX2).

It is also possible to list specific CPU features instead.
```
cargo multiarch --cpufeatures bmi,bmi2,avx2,avx512f
```

Important flags are forwarded to `cargo`, in particular be sure to not confuse package-level features `--features` and CPU features `--cpufeatures` (or `-c`)

### With Cargo.toml presets

`cargo-multiarch` can also read `Cargo.toml` for presets for example.

```toml
[package.metadata.multiarch.x86_64]
# x86-64-v1: Fallback with no features
# x86-64-v3: AVX2 CPUs (Intel Haswell 2013, AMD Excavator from 2015)
cpus = ["x86-64-v1", "x86-64-v3"]
cpufeatures = [[""]]
```

In that case, just call `cargo-multiarch` in the project root directory.
The presets can be overriden by CLI.

Presets cpufeatures, unlike in the CLI supports a list of lost of cpufeatures to build for, for example:
```toml
[package.metadata.multiarch.x86_64]
# x86-64-v1: Fallback with no features
# x86-64-v3: AVX2 CPUs (Intel Haswell 2013, AMD Excavator from 2015)
cpus = [""]
cpufeatures = [
    ["bmi", "bmi2", "avx2"],
    ["bmi", "bmi2", "avx2", "avx512f"],
]
```

Note that activating avx512f implies avx, avx2 and all SSE-levels, it may not imply non-SIMD feature sets like BMI and BMI2 (for bigint acceleration).
This should be tested.

## Limitations

This currently only works on Linux, Android, Solaris and most BSDs except MacOS which offer in-memory executable files.

Other platforms support can be easily added by using a regular temp file instead.

## Credits

This is a fork of [`cargo-multivers`](https://github.com/ronnychevalier/cargo-multivers).

Motivation and original design departures are documented in [./docs/design_space.md](docs/design_space.md)