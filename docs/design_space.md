# Design decisions

This library has been inspired by [cargo-multiverse](https://github.com/ronnychevalier/cargo-multivers)
which neatly solves shipping multiple versions of binaries compiled for different CPU features, for example:
- default
- AVX2
- AVX512

However the original library has a showstopper issue that prevents an optimized build from being ever used.

## The original library limitation

https://github.com/ronnychevalier/cargo-multivers/blob/f49d7b9/multivers-runner/src/build.rs#L48-L67

```Rust

    /// Finds a version that matches the CPU features of the host
    pub fn find_from(builds: impl IntoIterator<Item = Self>) -> Option<Self> {
        let supported_features: Vec<&str> = notstd_detect::detect::features()
            .filter_map(|(feature, supported)| supported.then_some(feature))
            .collect();

        builds.into_iter().find_map(|build| {
            build
                .features
                .iter()
                .all(|feature| supported_features.contains(feature))
                .then_some(build)
        })
    }

    /// Finds a version that matches the CPU features of the host
    pub fn find() -> Option<Self> {
        Self::find_from(PATCHES)
    }
```

The code does the following:
1. Detect all features of the CPU the code is currently running on
2. Check if among all the binaries built, there is one that match all the features
3. If yes, use this one, if not, use the default

The issue is that `notstd_detect::detect::features()` lists features
that don't match the feature sets associated to the binaries, for example `tsc` or `mmx`, so optimized builds are never used.

## Rewrite and design decisions

### Required feature declaration

Instead of specifying features for compilation target, cargo-multivers specifies CPUs,
but libraries are using target_feature not CPUs.
Also CPUs are problematic when some features like AVX512 get deactivated on later CPUs.

### Feature detection

We use notstd_detect instead of std::detect because
- std::detect uses removed feature const_fn
  and has seen no release since has_cpuid bug has been solved
  https://github.com/rust-lang/stdarch/issues/1526

### Binary selection

To avoid the original library issue we:
1. Filter all flavors that can run on our CPU by making sure all features required are available
2. Rank the features with a manually defined ranking
3. Launch the highest-ranked binary

### Binary diff

[qbsdiff](https://github.com/hucsmn/qbsdiff) seems to be the best API and maintained (it has examples) and highest rated (though 31 is low).
Alternatives are:
- [bidiff](https://github.com/divvun/bidiff) has higher rating and some fuzzing
  but no example no update for 5 years and using a niche compression/decompression backend
- [ddelta-rs](https://github.com/lights0123/ddelta-rs) has seen no update for 5 years
- [bita](https://github.com/oll3/bita) has the highest rating and maintenance but it's focused on synchronization and ships a webserver.

### Bundling binaries into a fatbin

The original cargo-multivers converts build artifacts to Rust's `Vec<u8>` as literal source code
and `include!` it.
This is an extra conversion step that is costly at compile-time (both time and size) but most efficient at run-time.

We seriously considered to serialize the build artifacts and deserialize them at run-time with serde's partial zero-copy with the `borrow` annotation:
```Rust
#[serde_as]
#[derive(Deserialize, Serialize)]
#[serde(bound(deserialize = "'de: 'a"))]
pub(crate) struct FatBin<'a> {
    #[serde_as(as = "Bytes")]
    #[serde(borrow = "'a")] // Partial Zero-copy
    default_exe: &'a [u8],
    #[serde(borrow = "'a")] // Partial Zero-copy
    pub(crate) features: Vec<Vec<&'a str>>,
    #[serde_as(as = "Vec<Bytes>")]
    #[serde(borrow = "'a")] // Partial Zero-copy
    patches: Vec<&'a [u8]>,
}
```

Compile-time speed should be improved _in theory_.
However `include_bytes!` from the standard library can still cause significant compile-time cost: https://github.com/rust-lang/rust/issues/65818, as just like regular Rust code, it parses the full bytes 1 by 1 to create documentation from it instead of just memcpy data.

Full zero copy alternatives are:
- [rkyv](https://github.com/rkyv/rkyv) which doesn't support `&[u8]` and `&str` and as we have nested `Vec<Vec<&str>>` we might lose inner zero-copy
- [zerocopy](https://github.com/google/zerocopy) does not support `Vec`
- [zerovec](https://github.com/unicode-org/icu4x/blob/3878cf1/utils/zerovec/design_doc.md)
  according to doc can emulate `Vec<Vec<u8>>` with `VarZeroVec<'a, ZeroSlice<u8>>`
  and `Vec<Vec<&'a str>>` with `VarZeroVec<'a, ZeroSlice<ZeroSlice<u8>>>`
  but it's quite verbose and it actually doesn't compile

The fastest zero-copy enabled serde backends are postcard and bincode 2, however bincode 2 is still in beta (and has been for the past 2 years)
We use [postcard](https://github.com/jamesmunns/postcard) as a serialization format.
`postcard` has significant development behind it, up to a stable wire format, and from rkyv's benchmark postcard is the fastest serde compatible serialization library across a wide range of data.

An alternative to include bytes without the compile-time cost is [include-blob!](https://github.com/SludgePhD/include-blob) but it uses arch/os dependent build script to pack data in object files, something tricky to test/debug and we would be its first big users.

### Temp files and execution


We need to store and patch the source the execute it.
Ideally we don't deal with the filesystem as it brings issues like disabled executable permissions in temporary directory.

On Linux, BSDs, Solaris, the usual technique is memfd + fexecve (to execute a file descriptor without path):
- https://github.com/novafacing/memfd-exec (more complex that necessary for our purpose)
- It does not exist on MacOS (and Haiku): https://github.com/rust-lang/libc/commit/7fd1f9d39bd7a2d40bbe02933e098eabe7310667

Other alternatives include:
- creating a temporary file and execute it. This may run into permissions issues
- create a shared library instead and dlopen it. This require shipping multiple files
- mmap/virtualalloc and copy the binary into executable memory. This requires creating a binary loader that will redo what Linux/Windows does before executing `main` (parsing file, loading dependencies, deal with memory relocations and absolute address jumps, ...)

It may be that Rust provides a native way to do this:
https://doc.rust-lang.org/nightly/cargo/reference/unstable.html?highlight=Bindeps#example-use-binary-artifact-and-its-library-in-a-binary
however it seems like the binary is not bundled?
