use phf::phf_map;

use super::{CpuFeatList, FatBin, FlavorsRank};

/// Ranking strategy
/// - We first map flavor instructions to a certain level
/// - then we pick the highest weight
/// - and if there are multiple features in the sam weight,
///   we pick the flavor with the highest count of top features
///
/// Example:
///   Bigint/elliptic curves code may be compiled with
///   - generic
///   - or BM1 (MULX)
///   - or BMI1 + BMI2 (ADOX, ADCX)
///   and all have significant performance profile (10~15% and 30% compared to baseline)
///   See table 2, p13 of https://raw.githubusercontent.com/wiki/intel/intel-ipsec-mb/doc/ia-large-integer-arithmetic-paper.pdf
///
///   Note: this is a contrived example as BMI1 and BMI2 shipped at the same time on Intel
///         and AMD CPUs had a small market share
///
/// The levels are provided by
///   https://en.wikipedia.org/wiki/X86-64#Microarchitecture_levels
/// The features can be listed with
///   rustc --print=target-features

struct Rank {
    level: usize,
    weight: usize,
}

const RANKING: phf::Map<&'static str, Rank> = phf_map! {
    "sse3"      => Rank{level: 2, weight: 1}, // Intel Q1 2004 Pentium 4,    AMD Q2 2005 Athlon 64 (Venice, San Diego)
    "ssse3"     => Rank{level: 2, weight: 2}, // Intel Q2 2006,              AMD Q4 2011 Bulldozer
    "sse4.1"    => Rank{level: 2, weight: 3}, // Intel Q4 2007 Penryn,       AMD Q4 2011 Bulldozer (note: AMD had SSE4a with part of 4.1)
    "popcnt"    => Rank{level: 2, weight: 4}, // Intel Q4 2008 Nehalem,      AMD Q4 2007 K10
    "sse4.2"    => Rank{level: 2, weight: 5}, // Intel Q4 2008 Nehalem,      AMD Q4 2011 Bulldozer (required by Windows 11 24H2)
    "avx"       => Rank{level: 3, weight: 1}, // Intel Q1 2011 Sandy Bridge, AMD Q4 2011 Bulldozer
    "avx2"      => Rank{level: 3, weight: 2}, // Intel Q2 2013 Haswell,      AMD Q2 2015 Excavator
    "lzcnt"     => Rank{level: 3, weight: 2}, // Intel Q2 2013 Haswell,      AMD Q2 2015 Excavator (2014, low-power Jaguar)
    "bmi"       => Rank{level: 3, weight: 2}, // Intel Q2 2013 Haswell,      AMD Q2 2015 Excavator (2014, low-power Jaguar)
    "bmi2"      => Rank{level: 3, weight: 2}, // Intel Q2 2013 Haswell,      AMD Q2 2015 Excavator
    // TODO: AVX-512 is a mess
    "avx512f"   => Rank{level: 4, weight: 1},
    "avx512cd"  => Rank{level: 4, weight: 1},
    "avx512vl"  => Rank{level: 4, weight: 2},
    "avx512dq"  => Rank{level: 4, weight: 2},
    "avx512bw"  => Rank{level: 4, weight: 2},
    // TODO: AVX256 IFMA are supported on Intel Alder lake or later, while AVX512 is not
    // TODO: where to put accelerators like:
    //   - AES, SHA256,
    //   - GFNI (Galois field new instructions for binary polynomial multiplication),
    //   - VPCLMULQDQ (vectorized Carryless mul)
};

impl<'a> FlavorsRank<'a> for FatBin<'a> {
    /// Returns the index of the top ranked set of x86 features.
    /// The Peek trait that allow checking emptiness
    /// requires a mutable reference to an iterator which a burdening constraint
    /// Hence we return -1 if the list is empty
    fn get_top_ranked(patches_features: impl Iterator<Item = CpuFeatList<'a>>) -> isize
    {
        let (top_idx, _, _, _) = patches_features.enumerate().fold(
            (-1isize, 0, 0, 0),
            |(top_index, top_level, top_weight, top_count), (index, patch_feats)| {
                let (bin_level, bin_weight, bin_count) =
                    patch_feats.0.iter().fold((0, 0, 0), |max, feature| {
                        let (max_level, max_weight, count) = max;
                        if let Some(Rank { level, weight }) = RANKING.get(feature) {
                            let (level, weight) = (*level, *weight);
                            if (level, weight) > (max_level, max_weight) {
                                (level, weight, 1)
                            } else if (level, weight) == (max_level, max_weight) {
                                (level, weight, count + 1)
                            } else {
                                (level, weight, count)
                            }
                        } else {
                            (max_level, max_weight, count)
                        }
                    });
                if (bin_level, bin_weight, bin_count) > (top_level, top_weight, top_count) {
                    (index as isize, bin_level, bin_weight, bin_count)
                } else {
                    (top_index, top_level, top_weight, top_count)
                }
            },
        );
        top_idx
    }
}
