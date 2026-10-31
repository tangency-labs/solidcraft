use serde_json::{Value, json};

use super::*;
use crate::cmd::{CommandSpec, find_command};

fn spec(id: &str) -> &'static CommandSpec {
    find_command(id).unwrap_or_else(|| panic!("no command `{id}` in the registry"))
}

fn command(access: &Access, id: &str) -> Result<(), String> {
    access.allows(Ask::Command(spec(id))).map_err(|why| why.to_string())
}

fn creator() -> Role {
    Role::built_in("Creator").unwrap()
}

fn worker() -> Role {
    Role::built_in("Worker").unwrap()
}

fn viewer() -> Role {
    Role::built_in("Viewer").unwrap()
}

#[test]
fn the_built_in_roles_are_creator_worker_and_viewer() {
    for name in ["Creator", "Worker", "Viewer"] {
        assert_eq!(Role::built_in(name).map(|r| r.name), Some(name.to_string()));
    }
    for name in ["Supplier", "creator", "VIEWER", "Creator ", ""] {
        assert_eq!(Role::built_in(name), None, "{name:?}");
    }
}

#[test]
fn a_viewer_cannot_change_the_design_and_a_creator_can() {
    assert_eq!(command(&Access::new(&viewer()), "solid.extrude"), Err("Viewers can't change the design".into()));
    assert_eq!(command(&Access::new(&creator()), "solid.extrude"), Ok(()));
}

/// The save, export and copy commands of the roles design (Q3) and the slicer and Printables
/// exports, written out here so the test doesn't read the list it checks.
const SAVE_EXPORT_COPY: [&str; 18] = [
    "file.save",
    "file.save_as",
    "file.export",
    "file.save_mesh",
    "file.send_to_slicer",
    "file.open_in_easyprint",
    "file.export_view_dxf",
    "sketch.export_dxf",
    "sheet.export_dxf",
    "sheet.export_bend_table",
    "parameters.export",
    "motion.export",
    "motion.export_animation",
    "timeline.export_script",
    "library.export",
    "render.export",
    "timeline.copy",
    "sketch.copy",
];

#[test]
fn a_viewer_runs_listed_reads_and_refuses_saves_exports_copies_and_hidden_edits() {
    let viewer = Access::new(&viewer());
    for id in ["inspect.measure", "view.fit", "select.set", "parameters.list", "browser.expand_all"] {
        assert_eq!(command(&viewer, id), Ok(()), "{id} is a read");
    }
    for id in SAVE_EXPORT_COPY {
        assert!(!spec(id).undoable, "{id} is listed because it makes no undo step");
        assert_eq!(command(&viewer, id), Err("Viewers can't save or export the design".into()), "{id}");
    }
    // A read that writes a file through `path`, the slicer list (slicing is refused), crash
    // reports (they hold other designs and paths), and commands that change the design or its
    // history without an undo step.
    let more = [
        "assembly.bom",
        "file.slicers",
        "diagnostics.crash_report",
        "diagnostics.crash_reports",
        "edit.undo",
        "edit.redo",
        "edit.restore_checkpoint",
    ];
    for id in more {
        assert!(!spec(id).undoable, "{id} is listed because it makes no undo step");
        assert_eq!(command(&viewer, id), Err(format!("Viewers can't use {}", spec(id).label)), "{id}");
    }
    // Undoable commands are refused as edits, `view.save` among them.
    assert_eq!(command(&viewer, "view.save"), Err("Viewers can't change the design".into()));
    for spec in crate::cmd::command_specs().into_iter().filter(|spec| spec.undoable) {
        assert_eq!(viewer.allows(Ask::Command(spec)).map_err(|why| why.to_string()), Err("Viewers can't change the design".into()), "{}", spec.id);
    }
}

/// Every command is decided for the Viewer: allowed as a read, or refused because it changes
/// the design (undoable) or because the Viewer's rules deny it by id. A command added later
/// fails here until someone decides it.
#[test]
fn every_command_is_decided_for_the_viewer() {
    let role = viewer();
    let viewer = Access::new(&role);
    let denied: Vec<&Pattern> = role
        .rules
        .iter()
        .filter_map(|rule| match (&rule.effect, &rule.what) {
            (Effect::Deny, What::Commands(p)) => Some(p),
            _ => None,
        })
        .collect();
    let undecided: Vec<&str> = crate::cmd::command_specs()
        .into_iter()
        .filter(|spec| viewer.allows(Ask::Command(spec)).is_err() && !spec.undoable && !denied.contains(&&Pattern::Exact(spec.id.into())))
        .map(|spec| spec.id)
        .collect();
    assert!(
        undecided.is_empty(),
        "not decided for the Viewer: {undecided:?}. Add each to VIEWER_READS (a read) or VIEWER_REFUSES (writes files or changes \
         the design without an undo step; a save or export goes in SAVES_OR_EXPORTS) in crates/engine/src/collaboration/role.rs"
    );
}

/// The Viewer names commands one by one, each registered: a pattern such as `view.*` would let a
/// later command in that area through undecided.
#[test]
fn the_viewer_names_registered_commands_one_by_one() {
    for rule in viewer().rules {
        match &rule.what {
            What::Commands(Pattern::Exact(id)) => {
                assert!(find_command(id).is_some_and(|c| c.id == id), "the Viewer names `{id}`, which isn't registered")
            }
            What::Commands(p) => panic!("the Viewer names commands by {p:?}, not one by one"),
            What::Ability(_) => {}
        }
    }
}

/// A second guard, from the control channel's own classifier: nothing the Viewer runs writes a
/// file when given a path.
#[test]
fn nothing_a_viewer_runs_writes_a_file() {
    let viewer = Access::new(&viewer());
    for spec in crate::cmd::command_specs().into_iter().filter(|spec| viewer.allows(Ask::Command(spec)).is_ok()) {
        let effect = crate::control_pending::command_effect(spec.id, &json!({"path": "out.file"}));
        assert_ne!(effect, crate::control_pending::Effect::WritesFile, "the Viewer may run {}, which writes a file", spec.id);
    }
}

#[test]
fn creators_and_workers_run_every_command() {
    for role in [creator(), worker()] {
        let access = Access::new(&role);
        for spec in crate::cmd::command_specs() {
            assert_eq!(access.allows(Ask::Command(spec)), Ok(()), "{} {}", role.name, spec.id);
        }
    }
}

#[test]
fn getting_the_design_means_seeing_it() {
    for role in [creator(), worker(), viewer()] {
        assert!(has(&role, Ability::See), "{} can't see", role.name);
    }
    let design_only = Role { name: "Reviewer".into(), rules: vec![allow(What::Ability(Ability::Design))] };
    assert!(has(&design_only, Ability::See));
    let meshes_only = Role { name: "Supplier".into(), rules: vec![allow(What::Ability(Ability::Meshes))] };
    assert!(!has(&meshes_only, Ability::See));
}

fn custom(rules: Vec<Rule>) -> Access {
    Access::new(&Role { name: "Supplier".into(), rules })
}

fn allow(what: What) -> Rule {
    Rule { effect: Effect::Allow, what, target: None, stand_in: false }
}

fn deny(what: What) -> Rule {
    Rule { effect: Effect::Deny, what, target: None, stand_in: false }
}

fn commands(text: &str) -> What {
    What::Commands(Pattern::parse(text))
}

#[test]
fn a_role_with_no_rules_is_refused_everything() {
    let none = custom(vec![]);
    assert_eq!(command(&none, "inspect.measure"), Err("The Supplier role can't use Measure".into()));
    assert!(crate::cmd::command_specs().into_iter().all(|spec| none.allows(Ask::Command(spec)).is_err()));
}

#[test]
fn deny_wins_over_allow_in_either_order() {
    for rules in [vec![allow(commands("solid.*")), deny(commands("solid.box"))], vec![deny(commands("solid.box")), allow(commands("solid.*"))]] {
        let access = custom(rules);
        assert_eq!(command(&access, "solid.box"), Err("The Supplier role can't use Box".into()));
        assert_eq!(command(&access, "solid.extrude"), Ok(()));
    }
    let no_edits = custom(vec![allow(commands("*")), deny(What::Ability(Ability::Edit))]);
    assert_eq!(command(&no_edits, "solid.extrude"), Err("The Supplier role can't change the design".into()));
    assert_eq!(command(&no_edits, "inspect.measure"), Ok(()));
    let no_saves = custom(vec![allow(commands("*")), deny(commands("file.save"))]);
    assert_eq!(command(&no_saves, "file.save"), Err("The Supplier role can't save or export the design".into()));
}

#[test]
fn an_exact_pattern_matches_one_id_and_a_prefix_every_id_that_starts_with_it() {
    let exact = custom(vec![allow(commands("solid.box"))]);
    assert_eq!(command(&exact, "solid.box"), Ok(()));
    assert!(command(&exact, "solid.extrude").is_err());
    // `solid.box` is not a prefix match for the exact pattern `solid.bo`.
    assert!(command(&custom(vec![allow(commands("solid.bo"))]), "solid.box").is_err());
    let prefix = custom(vec![allow(commands("solid.*"))]);
    assert_eq!(command(&prefix, "solid.box"), Ok(()));
    assert_eq!(command(&prefix, "solid.extrude"), Ok(()));
    assert!(command(&prefix, "sketch.line").is_err());
    assert_eq!(command(&custom(vec![allow(commands("solid.bo*"))]), "solid.box"), Ok(()));
}

#[test]
fn a_rule_with_a_target_allows_a_command_somewhere_but_refuses_it_only_there() {
    let housing = TargetRule { kind: TargetKind::Component, pattern: Pattern::Exact("Housing".into()) };
    let only_housing = custom(vec![allow(commands("*")), Rule { target: Some(housing.clone()), ..deny(What::Ability(Ability::Edit)) }]);
    assert_eq!(command(&only_housing, "solid.extrude"), Ok(()));
    let edits_on_housing = custom(vec![Rule { target: Some(housing), ..allow(commands("solid.*")) }]);
    assert_eq!(command(&edits_on_housing, "solid.extrude"), Ok(()));
}

#[test]
fn a_command_outside_the_registry_is_refused() {
    // A registered command under an id the registry doesn't have (not a spec of its own, which the
    // icon check would count as a command without an icon).
    let stray = CommandSpec { id: "stray.command", ..*command_specs()[0] };
    assert!(Access::new(&creator()).allows(Ask::Command(&stray)).is_err());
}

#[test]
fn creators_and_workers_get_the_design_and_viewers_meshes_only() {
    let gets =
        |role: Role| [Receive::Design, Receive::Meshes].map(|what| Access::new(&role).allows(Ask::Receive(what)).map_err(|why| why.to_string()));
    assert_eq!(gets(creator()), [Ok(()), Err("Creators can't get meshes only".into())]);
    assert_eq!(gets(worker()), [Ok(()), Err("Workers can't get meshes only".into())]);
    assert_eq!(gets(viewer()), [Err("Viewers can't get the design".into()), Ok(())]);
}

// The member list file.

/// An `ssh-ed25519` public key line whose 32 key bytes are all `n`.
fn key(n: u8, comment: &str) -> String {
    key_blob(&[&[0, 0, 0, 11], b"ssh-ed25519", &[0, 0, 0, 32], &[n; 32]], comment)
}

fn key_blob(parts: &[&[u8]], comment: &str) -> String {
    format!("ssh-ed25519 {} {comment}", solidcraft_doc::canvas::base64_encode(&parts.concat())).trim_end().to_string()
}

fn member(key: &str, role: &str) -> Value {
    json!({"key": key, "name": "Alex", "colour": "#3366ff", "role": role, "kind": "person", "added_by": self::key(1, "")})
}

/// A Creator, and an agent that views.
fn good() -> Value {
    json!({
        "format": "solidcraft-members/1",
        "version": 3,
        "members": [
            member(&key(1, "alex@example"), "Creator"),
            {"key": key(2, ""), "name": "Sam's agent", "colour": "#FF8800", "role": "Viewer", "kind": "agent", "added_by": key(1, "")}
        ]
    })
}

fn read(v: &Value) -> Result<MemberList, String> {
    MemberList::read(&serde_json::to_vec(v).unwrap()).map_err(|e| e.to_string())
}

fn read_text(text: &str) -> Result<MemberList, String> {
    MemberList::read(text.as_bytes()).map_err(|e| e.to_string())
}

/// `v` is refused, for the reason `why`.
#[track_caller]
fn refused(v: &Value, why: &str) {
    match read(v) {
        Ok(_) => panic!("accepted; expected a refusal with `{why}`"),
        Err(e) => assert!(e.contains(why), "refused with `{e}`; expected `{why}`"),
    }
}

/// `good()` changed by `edit`.
fn with(edit: impl FnOnce(&mut Value)) -> Value {
    let mut v = good();
    edit(&mut v);
    v
}

#[test]
fn a_member_list_is_read_with_its_members_and_their_built_in_roles() {
    let list = read(&good()).unwrap();
    assert_eq!(list.version, 3);
    assert_eq!(list.members.len(), 2);
    assert_eq!(list.members[1].kind, MemberKind::Agent);
    assert_eq!(list.members[1].name, "Sam's agent");
    assert_eq!(Role::built_in(&list.members[0].role), Some(creator()));
    assert_eq!(Role::built_in(&list.members[1].role), Some(viewer()));
}

#[test]
fn a_member_s_role_is_a_built_in_role() {
    for role in ["Supplier", "creator", "VIEWER", "Creator "] {
        refused(&with(|v| v["members"][1]["role"] = json!(role)), &format!("`{role}` isn't a role (one of Creator, Worker, Viewer)"));
    }
}

#[test]
fn custom_roles_and_invites_are_refused_until_they_are_built() {
    let supplier = json!([{"name": "Supplier", "rules": [{"effect": "allow", "what": {"ability": "meshes"}}]}]);
    refused(&with(|v| v["roles"] = supplier), "unknown field `roles`");
    refused(&with(|v| v["roles"] = json!([])), "unknown field `roles`");
    refused(&with(|v| v["invites"] = json!([{"role": "Viewer"}])), "unknown field `invites`");
    refused(&with(|v| v["invites"] = json!([])), "unknown field `invites`");
}

#[test]
fn a_member_list_up_to_one_mebibyte_is_read_and_a_larger_one_refused() {
    let mut bytes = serde_json::to_vec(&good()).unwrap();
    bytes.resize(1 << 20, b' ');
    assert!(MemberList::read(&bytes).is_ok());
    bytes.push(b' ');
    assert!(MemberList::read(&bytes).unwrap_err().to_string().contains("larger than 1 MiB"));
}

#[test]
fn anything_but_a_member_list_is_refused() {
    assert!(MemberList::read(b"not json").is_err());
    assert!(MemberList::read(b"").is_err());
    refused(&json!([1, 2]), "not a SolidCraft member list");
    refused(&with(|v| v["format"] = json!("solidcraft-members/2")), "format `solidcraft-members/2`");
    refused(&with(|v| v["format"] = json!("solidcraft/57")), "not a SolidCraft member list");
    refused(&with(|v| _ = v.as_object_mut().unwrap().remove("format")), "not a SolidCraft member list");
}

#[test]
fn an_unknown_field_anywhere_refuses_the_whole_list() {
    refused(&with(|v| v["extra"] = json!(1)), "unknown field `extra`");
    refused(&with(|v| v["members"][0]["admin"] = json!(true)), "unknown field `admin`");
    refused(&with(|v| v["members"][0]["kind"] = json!("robot")), "unknown variant `robot`");
}

#[test]
fn a_version_is_a_whole_number_from_0_to_u64_max() {
    assert!(read(&with(|v| v["version"] = json!(u64::MAX))).is_ok());
    assert!(read(&with(|v| v["version"] = json!(0))).is_ok());
    for version in ["-1", "1.5", "18446744073709551616", "1e3", "\"3\"", "null"] {
        let text = serde_json::to_string(&good()).unwrap().replace("\"version\":3", &format!("\"version\":{version}"));
        assert!(read_text(&text).is_err(), "version {version} was accepted");
    }
}

/// serde_json refuses a field given twice in a typed record (it doesn't keep the first or the
/// last), so a member can't carry two roles.
#[test]
fn a_field_given_twice_refuses_the_whole_list() {
    let text = serde_json::to_string(&good()).unwrap();
    let twice = text.replacen("\"role\":\"Viewer\"", "\"role\":\"Viewer\",\"role\":\"Creator\"", 1);
    assert!(read_text(&twice).is_err_and(|e| e.contains("duplicate field `role`")), "{twice}");
    let twice = text.replacen("\"version\":3", "\"version\":3,\"version\":4", 1);
    assert!(read_text(&twice).is_err_and(|e| e.contains("duplicate field `version`")));
}

#[test]
fn deep_nesting_is_refused_not_a_crash() {
    for depth in [200, 100_000] {
        let deep = format!("{}{}", "[".repeat(depth), "]".repeat(depth));
        let text = serde_json::to_string(&good()).unwrap().replacen('{', &format!("{{\"deep\":{deep},"), 1);
        assert!(read_text(&text).is_err(), "depth {depth}");
    }
}

#[test]
fn up_to_1024_members_are_read_and_1025_refused() {
    let members = |n: usize| {
        (0..n)
            .map(|i| member(&key_blob(&[&[0, 0, 0, 11], b"ssh-ed25519", &[0, 0, 0, 32], &[0; 30], &(i as u16).to_be_bytes()], ""), "Creator"))
            .collect::<Vec<_>>()
    };
    let list = |n: usize| {
        with(|v| {
            v["members"] = json!(members(n));
            v["members"][0]["key"] = json!(key(1, ""));
        })
    };
    assert!(read(&list(1024)).is_ok());
    refused(&list(1025), "more than 1024 members");
}

#[test]
fn a_member_s_name_is_trimmed_1_to_64_characters_with_no_control_bidi_or_invisible_characters() {
    // A visible narrow space, a heart with its emoji variation selector, and the characters just
    // outside each refused range (U+E0100 is a CJK variation selector).
    let shown = ["é".repeat(64), "A".into(), "Sam's agent".into(), "Al\u{202F}ex".into(), "Sam \u{2764}\u{FE0F}".into(), "Al\u{2070}ex".into()];
    for name in shown.into_iter().chain("\u{AE}\u{180A}\u{2010}\u{2027}\u{FFF8}\u{E0100}".chars().map(|c| format!("Al{c}ex"))) {
        assert!(read(&with(|v| v["members"][0]["name"] = json!(name))).is_ok(), "{name:?}");
    }
    let why = "a name must be trimmed, 1 to 64 characters";
    for name in ["é".repeat(65), String::new(), " ".into(), " Alex".into(), "Alex ".into(), "Al\u{0}ex".into(), "Al\nex".into()] {
        refused(&with(|v| v["members"][0]["name"] = json!(name)), why);
    }
    // Soft hyphen, grapheme joiner, Arabic letter mark, fillers, Mongolian selectors, zero-width
    // spaces and joiners, bidi marks, line and paragraph separators, bidi embeddings, word joiners
    // and isolates (with the reserved invisible between them), BOM, annotations, musical format
    // characters, tags.
    let hidden = "\u{AD}\u{34F}\u{61C}\u{115F}\u{1160}\u{17B4}\u{17B5}\u{180B}\u{180F}\u{200B}\u{200D}\u{200E}\u{200F}\u{2028}\u{2029}\u{202A}\u{202E}\
                  \u{2060}\u{2065}\u{2066}\u{2069}\u{206F}\u{3164}\u{FEFF}\u{FFA0}\u{FFF9}\u{FFFB}\u{1D173}\u{1D17A}\u{E0001}\u{E0041}\u{E007F}";
    for c in hidden.chars() {
        refused(&with(|v| v["members"][0]["name"] = json!(format!("Al{c}ex"))), why);
    }
}

#[test]
fn a_member_whose_name_an_earlier_member_has_must_choose_another() {
    let list = |names: &[&str]| {
        let members: Vec<_> = names
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let mut m = member(&key(i as u8 + 1, ""), "Creator");
                m["name"] = json!(name);
                m
            })
            .collect();
        read(&with(|v| v["members"] = json!(members))).unwrap()
    };
    assert_eq!(list(&["Alex", "Sam", "Al ex"]).renames_needed(), Vec::<usize>::new());
    // The file is still read; only those who joined after the first holder of a name must change.
    assert_eq!(list(&["Alex", "Sam", "alex", "ALEX", "Sam"]).renames_needed(), [2, 3, 4]);
}

#[test]
fn a_member_s_colour_is_hash_rrggbb() {
    for colour in ["#3366ff", "#3366FF", "#000000"] {
        assert!(read(&with(|v| v["members"][0]["colour"] = json!(colour))).is_ok(), "{colour}");
    }
    for colour in ["3366ff", "#3366f", "#3366fff", "#33669g", " #3366ff", "#3366ff ", "red", "", "#", "#３３６６ｆｆ"] {
        refused(&with(|v| v["members"][0]["colour"] = json!(colour)), "colour must be #rrggbb");
    }
}

#[test]
fn a_member_s_key_role_and_adder_are_1_to_256_bytes_with_no_control_characters() {
    let why = "must be 1 to 256 bytes";
    for field in ["key", "role", "added_by"] {
        // 256 bytes passes this check (and then fails the key or role check).
        if let Err(e) = read(&with(|v| v["members"][1][field] = json!("k".repeat(256)))) {
            assert!(!e.contains(why), "{field}: {e}");
        }
        refused(&with(|v| v["members"][1][field] = json!("")), why);
        refused(&with(|v| v["members"][1][field] = json!("k".repeat(257))), why);
        refused(&with(|v| v["members"][1][field] = json!("é".repeat(128) + "k")), why);
        refused(&with(|v| v["members"][1][field] = json!("a\tb")), why);
        refused(&with(|v| v["members"][1][field] = json!("a\u{7f}")), why);
    }
}

#[test]
fn a_key_is_an_ssh_ed25519_public_key() {
    assert!(read(&with(|v| v["members"][1]["key"] = json!(key(2, "sam@laptop")))).is_ok());
    let why = "key isn't an ssh-ed25519 public key";
    let ed = b"ssh-ed25519";
    for bad_key in [
        "ssh-ed25519".to_string(),
        "ssh-ed25519 !!!!".into(),
        key(2, "").replace("ssh-ed25519", "ssh-rsa"),
        key_blob(&[&[0, 0, 0, 11], ed, &[0, 0, 0, 31], &[2; 31]], ""),
        key_blob(&[&[0, 0, 0, 11], ed, &[0, 0, 0, 33], &[2; 33]], ""),
        key_blob(&[&[0, 0, 0, 11], ed, &[0, 0, 0, 32], &[2; 32], &[0]], ""),
        key_blob(&[&[0, 0, 0, 7], b"ssh-rsa", &[0, 0, 0, 32], &[2; 32]], ""),
        key_blob(&[&[0, 0, 0, 11], ed, &[0xff, 0xff, 0xff, 0xff], &[2; 32]], ""),
        key_blob(&[&[0xff, 0xff, 0xff, 0xff], ed], ""),
        key_blob(&[&[0, 0, 0]], ""),
    ] {
        refused(&with(|v| v["members"][1]["key"] = json!(bad_key)), why);
    }
}

#[test]
fn one_key_is_one_member_however_it_is_written() {
    refused(&with(|v| v["members"][1]["key"] = json!(key(1, ""))), "two members have the key");
    refused(&with(|v| v["members"][1]["key"] = json!(key(1, "another comment"))), "two members have the key");
}

#[test]
fn a_member_is_added_by_a_member() {
    assert!(read(&with(|v| v["members"][1]["added_by"] = json!(key(2, "themself")))).is_ok());
    let why = "added_by isn't a member's key";
    refused(&with(|v| v["members"][1]["added_by"] = json!(key(9, ""))), why);
    refused(&with(|v| v["members"][0]["added_by"] = json!("ssh-ed25519 !!!!")), why);
}

#[test]
fn a_list_where_nobody_can_manage_people_and_roles_is_refused() {
    let why = "nobody in it can manage people and roles";
    refused(&with(|v| v["members"][0]["role"] = json!("Worker")), why);
    refused(&with(|v| v["members"] = json!([])), why);
    assert!(read(&with(|v| v["members"][1]["role"] = json!("Worker"))).is_ok());
}

/// The roles design's A1 fuzz: mutations of 20 valid lists are each refused whole or read,
/// never a panic, within testkit's time budget, and a list that is read names only built-in
/// roles.
#[test]
fn mutated_member_lists_are_refused_whole_or_read() {
    let mut repeated = 0;
    for seed in 1..=20u8 {
        let count = usize::from(seed);
        let members: Vec<Value> = (0..count)
            .map(|i| {
                let role = ["Creator", "Worker", "Viewer"][i % 3];
                let kind = ["person", "agent"][i % 2];
                json!({"key": key(i as u8 + 1, ["", "a@b"][i % 2]), "name": format!("Member {i}"), "colour": "#a0b1c2", "role": role, "kind": kind, "added_by": key(1, "")})
            })
            .collect();
        let list = json!({"format": "solidcraft-members/1", "version": seed, "members": members});
        let bytes = serde_json::to_vec_pretty(&list).unwrap();
        assert!(MemberList::read(&bytes).is_ok(), "list {seed} is valid");
        let cases = solidcraft_testkit::mutations(&bytes, true, u64::from(seed));
        solidcraft_testkit::survives(&format!("member list {seed}"), &cases, MAX_MEMBERS, |b| {
            MemberList::read(b).ok().map(|l| {
                assert!(l.members.iter().all(|m| Role::built_in(&m.role).is_some()), "a list naming another role was read");
                // Exactly the members whose name, ignoring case, someone earlier has.
                let earlier = |i: usize| l.members[..i].iter().any(|m| m.name.to_lowercase() == l.members[i].name.to_lowercase());
                let renames = l.renames_needed();
                assert_eq!(renames, (0..l.members.len()).filter(|&i| earlier(i)).collect::<Vec<_>>());
                repeated += usize::from(!renames.is_empty());
                l.members.len()
            })
        });
    }
    // The mutator must reach a repeated name, not only lists whose names all differ.
    assert!(repeated > 0, "no mutated list repeated a name");
}
