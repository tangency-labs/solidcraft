// SPDX-License-Identifier: Apache-2.0

use super::super::cosmetic_thread_extent;
use cadmpeg_ir::features::CosmeticThreadExtent;
use cadmpeg_ir::scalar::PositiveLength;

#[test]
fn the_full_length_flag_decides_a_through_thread_before_its_length_owner() {
    let length = PositiveLength::new(20.0);
    let blind = Some(CosmeticThreadExtent::Blind {
        length: length.unwrap(),
    });
    let through = Some(CosmeticThreadExtent::Through {});
    assert_eq!(cosmetic_thread_extent(Some(true), length, false), through);
    assert_eq!(cosmetic_thread_extent(Some(false), length, true), blind);
    assert_eq!(cosmetic_thread_extent(None, length, true), blind);
    assert_eq!(cosmetic_thread_extent(Some(false), None, true), through);
    assert_eq!(cosmetic_thread_extent(None, None, false), None);
}
