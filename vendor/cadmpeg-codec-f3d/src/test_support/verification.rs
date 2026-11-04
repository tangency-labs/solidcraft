// SPDX-License-Identifier: Apache-2.0
//! Select synthetic regression fixtures or an explicitly supplied private set.

fn private_path(relative: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("CADMPEG_F3D_VERIFICATION_FIXTURES")
        .map(|root| std::path::PathBuf::from(root).join(relative))
}

pub(crate) fn text(relative: &str, synthetic: &str) -> String {
    private_path(relative).map_or_else(
        || synthetic.to_owned(),
        |path| std::fs::read_to_string(path).expect("private verification fixture"),
    )
}

pub(crate) fn bytes(relative: &str, synthetic: &[u8]) -> Vec<u8> {
    private_path(relative).map_or_else(
        || synthetic.to_vec(),
        |path| std::fs::read(path).expect("private verification fixture"),
    )
}

pub(crate) fn case_count(synthetic: usize, private: usize) -> usize {
    if std::env::var_os("CADMPEG_F3D_VERIFICATION_FIXTURES").is_some() {
        private
    } else {
        synthetic
    }
}
