// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::{observe_f3z_members, F3zMemberObserver, F3zPhase};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Default)]
struct Events(RefCell<Vec<String>>);
impl F3zMemberObserver for Events {
    fn phase_started(&self, phase: F3zPhase) {
        self.0.borrow_mut().push(format!("start {phase:?}"));
    }
    fn phase_finished(&self, phase: F3zPhase, outcome: Result<(), &cadmpeg_core::CodecError>) {
        self.0
            .borrow_mut()
            .push(format!("finish {phase:?} {}", outcome.is_ok()));
    }
    fn member_started(&self, path: &str) {
        self.0.borrow_mut().push(format!("member {path}"));
    }
    fn member_finished(&self, _path: &str, _outcome: Result<(), &cadmpeg_core::CodecError>) {}
}

#[test]
fn archive_observer_preparation_precedes_member_decode() {
    let bytes = archive();
    let events = Rc::new(Events::default());
    observe_f3z_members(events.clone(), || {
        F3dCodec
            .decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
            .unwrap();
    });
    let events = events.0.borrow();
    assert_eq!(
        &events[..6],
        [
            "start Scan",
            "finish Scan true",
            "start Classify",
            "finish Classify true",
            "start SessionOpen",
            "finish SessionOpen true",
        ]
    );
    assert_eq!(events[6], "member root.f3d");
    assert!(events[6..].iter().all(|event| event.starts_with("member ")));
}

#[test]
fn archive_session_crc_failures_keep_unreachable_losses_and_root_refusal() {
    let root = f3d_without_brep("part-design", "root.f3d", &[]);
    let original = f3z_archive("root.f3d", &[("root.f3d", &root), ("unused.f3d", &root)]);
    for name in ["root.f3d", "unused.f3d"] {
        let mut bytes = original.clone();
        let offset = {
            let arena = DecodeArena::new();
            let (ctx, view) =
                DecodeContext::from_root_bytes(&bytes, &arena, &DecodeOptions::default().policy)
                    .unwrap();
            let archive = cadmpeg_container::ArchiveSnapshot::new(&ctx, view).unwrap();
            usize::try_from(archive.entry(name).unwrap().data_start.unwrap()).unwrap()
        };
        bytes[offset] ^= 1;
        let result = F3dCodec.decode(&mut Cursor::new(&bytes), &DecodeOptions::default());
        if name == "root.f3d" {
            assert!(result.is_err());
        } else {
            let result = result.unwrap();
            for code in [
                crate::F3dLossCode::ArchiveEntryUnreadable,
                crate::F3dLossCode::XrefMemberUndecoded,
            ] {
                assert!(result
                    .report()
                    .losses
                    .iter()
                    .any(|loss| loss.code == code.note("").code));
            }
        }
    }
}
