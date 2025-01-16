use std::collections::{btree_set, BTreeSet, HashMap};
use std::str::FromStr;

use cargo_metadata;
use itertools::Itertools;
use serde::{Deserialize, Deserializer};
use serde_json;
use target_lexicon::{Architecture, Triple};

use crate::rustc_queries::Rustc;

// Dealing with the orphan rule is such a pain ....

#[derive(PartialEq, Eq, Hash, Debug)]
#[repr(transparent)] // transmute safe
struct ArchitectureWrapper(Architecture);

impl<'de> Deserialize<'de> for ArchitectureWrapper {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let str = String::deserialize(deserializer)?;
        let arch = Architecture::from_str(&str).map_err(|_| {
            serde::de::Error::invalid_value(
                serde::de::Unexpected::Other(&str),
                &"a CPU architecture",
            )
        })?;
        Ok(Self(arch))
    }
}

impl From<Architecture> for ArchitectureWrapper {
    fn from(orig: Architecture) -> ArchitectureWrapper {
        ArchitectureWrapper(orig)
    }
}

impl<'a> From<&'a Architecture> for &'a ArchitectureWrapper {
    fn from(orig: &'a Architecture) -> &'a ArchitectureWrapper {
        unsafe { std::mem::transmute(orig) }
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Debug)]
#[repr(transparent)]
pub(crate) struct CpuFeatures(BTreeSet<String>);

impl FromIterator<String> for CpuFeatures {
    fn from_iter<I: IntoIterator<Item = String>>(iter: I) -> Self {
        Self(BTreeSet::from_iter(iter))
    }
}

impl CpuFeatures {
    pub(crate) fn iter(&self) -> btree_set::Iter<'_, String> {
        self.0.iter()
    }

    /// Builds a string of CPU feature flags that can be given to `rustc -C target-feature=` (e.g., `+aes,+avx,+sse`)
    pub fn to_compiler_flags(&self) -> String {
        if !self.0.is_empty() {
            ["+", &self.0.iter().join(",+")].concat()
        } else {
            String::new()
        }
    }
}

/// cargo-multiarch will compile a binary
/// - per cpu
/// - and per set of CPU features
#[derive(PartialEq, Eq, Hash, Debug, Clone, Deserialize)]
struct ConfigTargetsForArch {
    cpus: BTreeSet<String>,
    // a single <feature list> MUST be sorted and ideally deduped
    // and the list of <feature list> might as well be
    cpufeatures_lists: BTreeSet<CpuFeatures>,
}

pub(crate) struct ConfigMultiArch {
    target: Triple,
    archs: HashMap<ArchitectureWrapper, ConfigTargetsForArch>,
}

impl ConfigMultiArch {
    pub(crate) fn new(target: Triple) -> Self {
        Self {
            target,
            archs: Default::default(),
        }
    }
    pub(crate) fn load_cargo_toml(
        mut self,
        toml: &cargo_metadata::Package,
    ) -> anyhow::Result<Self> {
        if toml.metadata.is_null() {
            return Ok(self);
        };

        let metadata: HashMap<String, serde_json::Value> =
            serde_json::from_value(toml.metadata.clone())?;
        let Some(multiarch) = metadata.get("multiarch") else {
            return Ok(self);
        };
        let archs: HashMap<ArchitectureWrapper, ConfigTargetsForArch> =
            Deserialize::deserialize(multiarch)?;

        self.archs = archs;
        Ok(self)
    }

    pub(crate) fn override_cpus(mut self, cpus: BTreeSet<String>) -> anyhow::Result<Self> {
        if cpus.is_empty() {
            return Ok(self);
        };

        let arch = &self.target.architecture;

        if let Some(target_config) = self.archs.get_mut(arch.into()) {
            target_config.cpus = cpus;
        } else {
            let config_arch = ConfigTargetsForArch {
                cpus,
                cpufeatures_lists: BTreeSet::new(),
            };
            let _ = self.archs.insert((*arch).into(), config_arch);
        };
        Ok(self)
    }

    pub(crate) fn override_features_lists(
        mut self,
        cpufeat_lists: BTreeSet<CpuFeatures>,
    ) -> anyhow::Result<Self> {
        if cpufeat_lists.is_empty() {
            return Ok(self);
        };

        let arch = &self.target.architecture;

        if let Some(target_config) = self.archs.get_mut(arch.into()) {
            target_config.cpufeatures_lists = cpufeat_lists;
        } else {
            let config_arch = ConfigTargetsForArch {
                cpus: BTreeSet::new(),
                cpufeatures_lists: cpufeat_lists,
            };
            let _ = self.archs.insert((*arch).into(), config_arch);
        };
        Ok(self)
    }

    /// Retrieve the list of target features.
    /// If a cpu like x86-64-v3 was passed, it is converted to a list of features.
    /// The returned list is sorted and deduplicated at 2 level:
    /// - the inner list of features per build
    /// - the outer list of builds
    pub(crate) fn get_cpu_features(&self) -> BTreeSet<CpuFeatures> {
        let Some(target_config) = self.archs.get((&self.target.architecture).into()) else {
            return BTreeSet::new();
        };

        let features_of_cpus: BTreeSet<CpuFeatures> = target_config
            .cpus
            .iter()
            .flat_map(|cpu| {
                Rustc::get_cpufeatures_for_programs(Some(&self.target.to_string()), Some(&cpu))
                    .map(|list| CpuFeatures::from_iter(list))
            })
            .collect();

        if target_config.cpufeatures_lists.is_empty() {
            return features_of_cpus;
        } else {
            return features_of_cpus
                .union(&target_config.cpufeatures_lists)
                .cloned()
                .collect();
        }
    }
}
