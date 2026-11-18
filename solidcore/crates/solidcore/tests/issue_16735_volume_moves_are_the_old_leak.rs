//! #16735: every volume `brep_volume`'s closure of tolerant edges moved is the old
//! integrator's leak through the edges' gaps, not a change of shape (see
//! `support/volume_leak.rs` for the three checks).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test code

#[path = "support/volume_leak.rs"]
mod volume_leak;

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::brep_data::BrepData;
use solidcore::topology::brep_data_write::FromBrepEnv;
use solidcore::topology::database_io::FileType;
use solidcore::topology::part_file::{read_part_from_file, PartEnv};
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{Brep, EdgeId, EdgeuseId, FaceId};
use solidcore::{EntityRef, Kernel};

/// The checks on a box, whose edges are exact: no gap, no leak, the closed form.
#[test]
fn a_box_has_no_leak() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(1.0, -2.0, 3.0), 4.0, 5.0, 6.0).unwrap();
    let l = volume_leak::leak(&k, &mut b, 1e-10, 64);
    assert!((l.volume - 120.0).abs() <= 1e-9 * 120.0, "{l:?}");
    assert!(l.origin_spread <= l.origin_allowance, "{l:?}");
    assert!(
        l.old_bound <= 1e-9 && l.worst_gap_over_tolerance <= 1.0,
        "{l:?}"
    );
}

// ---- corpus.parity ----------------------------------------------------------------------

/// `corpus.parity`'s moved rows (#16735, from the goldens' diff): the model file under
/// `upstream/TestFiles/pt_TestFiles` (or the repository), the B-rep's index in it, and the
/// volume before and after, as read and after an ASCII round trip, at the unit's accuracy
/// 1e-6. No other value of these cases moved: their counts, kinds, check lines and areas
/// are unchanged in the diff.
const CORPUS_ROWS: [(&str, usize, f64, f64, f64, f64); 116] = [
    (
        "Defeature/LShape_I.smb",
        0,
        240700.10843641515,
        240700.10843641564,
        240700.10843641515,
        240700.10843641564,
    ),
    (
        "Defeature/LShape_II.smb",
        0,
        240700.10843641515,
        240700.10843641564,
        240700.10843641515,
        240700.10843641564,
    ),
    (
        "Defeature/Platte.smb",
        0,
        1311471.8598605434,
        1311471.8591890652,
        1311471.8598605327,
        1311471.8591890545,
    ),
    (
        "Defeature/brep1.smb",
        0,
        719.7247986254079,
        719.7247986254076,
        719.7247986254079,
        719.7247986254076,
    ),
    (
        "Defeature/extrusion.smb",
        0,
        591.8357935230322,
        591.8357935230324,
        591.8357935230322,
        591.8357935230324,
    ),
    (
        "Defeature/flathead_screw_top.smb",
        0,
        101.15312477815387,
        101.1531247781539,
        101.15312477815387,
        101.1531247781539,
    ),
    (
        "Defeature/inset_cube.smb",
        0,
        7.0,
        7.000000000000001,
        7.0,
        7.000000000000001,
    ),
    (
        "Fillet/0311.smb",
        0,
        20178295251.79182,
        20178295251.791813,
        20178295251.791817,
        20178295251.791813,
    ),
    (
        "Fillet/Heart.smb",
        0,
        326.2256679721579,
        326.2256679721581,
        326.2256679721579,
        326.2256679721581,
    ),
    (
        "Fillet/bague.smb",
        0,
        225.66888677514265,
        225.66888677514262,
        225.66888677514265,
        225.66888677514262,
    ),
    (
        "Fillet/blade_fillet_fails.smb",
        0,
        559948.7602124158,
        559949.7208505041,
        559948.7602124158,
        559949.7208505041,
    ),
    (
        "Fillet/blade_hub_1.smb",
        0,
        13864.930224871418,
        13864.897541866114,
        13864.930224871603,
        13864.897541866243,
    ),
    (
        "Fillet/blade_hub_2.smb",
        0,
        12399.559074022327,
        12399.55533471774,
        12399.55907402222,
        12399.555334717652,
    ),
    (
        "Fillet/blade_hub_3.smb",
        0,
        13013.302436946566,
        13013.30641059861,
        13013.30243694698,
        13013.306410599022,
    ),
    (
        "Fillet/bolt.smb",
        0,
        208879.65776532915,
        208880.6220194911,
        208879.65776532923,
        208880.62201949122,
    ),
    (
        "Fillet/bottle.smb",
        0,
        11.066834232209219,
        11.066801281974822,
        11.066834232209267,
        11.066801281974868,
    ),
    (
        "Fillet/box_diff_3cyls.smb",
        0,
        3123766.9677070347,
        3123766.9677070356,
        3123766.9677070347,
        3123766.9677070356,
    ),
    (
        "Fillet/box_extrusion2.smb",
        0,
        3800.1218572503312,
        3800.113897799012,
        3800.121857250331,
        3800.1138977990117,
    ),
    (
        "Fillet/box_w_slot.smb",
        0,
        0.6807434687656769,
        0.6807481065103066,
        0.6807434687656772,
        0.6807481065103067,
    ),
    (
        "Fillet/brepForFillet1.smb",
        0,
        1560.0000000930586,
        1560.0000000930563,
        1560.0000000930586,
        1560.0000000930563,
    ),
    (
        "Fillet/brepForFillet2.smb",
        0,
        3616.411473193866,
        3616.4114731938644,
        3616.411473193866,
        3616.4114731938644,
    ),
    (
        "Fillet/brick.smb",
        0,
        294967.15200000093,
        294967.15199999994,
        294967.15200000093,
        294967.15199999994,
    ),
    (
        "Fillet/cliff_1.smb",
        0,
        13864.930224871418,
        13864.897541866114,
        13864.930224871603,
        13864.897541866243,
    ),
    (
        "Fillet/cliff_2.smb",
        0,
        12399.559074022327,
        12399.55533471774,
        12399.55907402222,
        12399.555334717652,
    ),
    (
        "Fillet/cliff_3.smb",
        0,
        253591.0100776666,
        253591.01007766664,
        253591.0100776666,
        253591.01007766664,
    ),
    (
        "Fillet/cliff_4.smb",
        0,
        209463.86145516235,
        209463.8614551623,
        209463.86145516235,
        209463.8614551623,
    ),
    (
        "Fillet/cone_34.smb",
        0,
        252384.45741365798,
        252384.45501513904,
        252384.45741365798,
        252384.455015139,
    ),
    (
        "Fillet/corner_box.smb",
        0,
        456.17129734220765,
        456.1712723752877,
        456.17129734220765,
        456.1712723752877,
    ),
    (
        "Fillet/crown.smb",
        0,
        88.37196089732787,
        88.37196038492868,
        88.37196089732791,
        88.37196038492874,
    ),
    (
        "Fillet/cube_sphere2.smb",
        0,
        26.048028587330993,
        26.048029042826105,
        26.048028587330993,
        26.048029042826105,
    ),
    (
        "Fillet/cyl_cyl_219.smb",
        0,
        2986.0295196334487,
        2986.053529684314,
        2986.0295196334487,
        2986.053529684314,
    ),
    (
        "Fillet/cyl_cyl_2191.smb",
        0,
        2984.7086804092933,
        2984.7326910842125,
        2984.7086804092933,
        2984.732691084213,
    ),
    (
        "Fillet/cyl_on_box_w_fillet.smb",
        0,
        133.74446782769022,
        133.74446782769024,
        133.74446782769022,
        133.74446782769024,
    ),
    (
        "Fillet/cyl_on_box_w_fillet1.smb",
        0,
        133.74446782769024,
        133.74446782769007,
        133.74446782769024,
        133.74446782769007,
    ),
    (
        "Fillet/cyl_on_box_w_fillet2.smb",
        0,
        1088.5176891959713,
        1088.5176891959788,
        1088.5176891959713,
        1088.5176891959788,
    ),
    (
        "Fillet/cyl_on_box_w_hole.smb",
        0,
        4819172.092176401,
        4819172.092176403,
        4819172.092176401,
        4819172.092176403,
    ),
    (
        "Fillet/cyl_on_box_w_tan_hole.smb",
        0,
        1685084.9530342761,
        1685084.9530342752,
        1685084.9530342761,
        1685084.9530342752,
    ),
    (
        "Fillet/cyl_sphere.smb",
        0,
        269.3915996649788,
        269.3915996649792,
        269.3915996649788,
        269.3915996649792,
    ),
    (
        "Fillet/extrusion1002.smb",
        0,
        4539.21004236946,
        4539.210042369459,
        4539.21004236946,
        4539.210042369459,
    ),
    (
        "Fillet/fillet0114.smb",
        0,
        43416.117804687565,
        43416.11780468749,
        43416.117804687565,
        43416.11780468749,
    ),
    (
        "Fillet/fillet0227.smb",
        0,
        289438095479.7785,
        289437928629.3002,
        289438095479.7785,
        289437928629.3002,
    ),
    (
        "Fillet/fillet1010.smb",
        0,
        347499.9893877836,
        347499.9893878339,
        347499.9893877836,
        347499.9893878339,
    ),
    (
        "Fillet/fillet1017.smb",
        0,
        591203.6360092234,
        591204.1256279112,
        591203.6360092234,
        591204.1256279112,
    ),
    (
        "Fillet/fillet1018.smb",
        0,
        314072.84143600735,
        314072.8459869012,
        314072.84143600735,
        314072.8459869012,
    ),
    (
        "Fillet/fillet1019.smb",
        0,
        59314.64523001631,
        59314.63915396819,
        59314.64523001631,
        59314.639153968186,
    ),
    (
        "Fillet/filleted_box.smb",
        0,
        991.4159279220164,
        991.4159279220243,
        991.4159279220164,
        991.4159279220243,
    ),
    (
        "Fillet/knife.smb",
        0,
        2917.0668806755275,
        2917.067802158053,
        2917.0668806755275,
        2917.067802158053,
    ),
    (
        "Fillet/loft_brick.smb",
        0,
        28933.546433866515,
        28933.54643386651,
        28933.546433866515,
        28933.54643386651,
    ),
    (
        "Fillet/nurb_box1.smb",
        0,
        3802.7909140154234,
        3802.796324356901,
        3802.7909140154234,
        3802.796324356902,
    ),
    (
        "Fillet/nurb_cone.smb",
        0,
        213.72140659300896,
        213.72140659308954,
        213.72140659300896,
        213.7214065930895,
    ),
    (
        "Fillet/rollover_corner.smb",
        0,
        722078.0991000877,
        722078.2340235466,
        722078.0991000877,
        722078.2340235466,
    ),
    (
        "Fillet/sphere_diff_3cylinders.smb",
        0,
        169497.32057334777,
        169499.15078586998,
        169497.32057334777,
        169499.15078586998,
    ),
    (
        "Fillet/test1.smb",
        0,
        811.4006062393719,
        811.4006005595088,
        811.4006062393711,
        811.4006005595081,
    ),
    (
        "Fillet/test4.smb",
        0,
        645.1938979619962,
        645.1938973340342,
        645.1938979619963,
        645.1938973340343,
    ),
    (
        "Fillet/test6.smb",
        0,
        349.5132697374205,
        349.5132697384093,
        349.5132697374205,
        349.5132697384093,
    ),
    (
        "Fillet/test9.smb",
        0,
        430.4748093515858,
        430.47480935158615,
        430.47480935158586,
        430.47480935158615,
    ),
    (
        "Fillet/tilt_cyl_on_box.smb",
        0,
        1393240.517019337,
        1393240.5677354054,
        1393240.517019337,
        1393240.5677354054,
    ),
    (
        "Fillet/torii_fillet.smb",
        0,
        3937.4426916134553,
        3937.4425082478333,
        3937.4426916134553,
        3937.442508247833,
    ),
    (
        "Fillet/torii_unioned.smb",
        0,
        8905.312201002616,
        8905.27055426405,
        8905.312201002613,
        8905.270554264049,
    ),
    (
        "Fillet/vase.smb",
        0,
        615795.651716744,
        615798.2191727235,
        615795.651716744,
        615798.2191727235,
    ),
    (
        "Fillet/vase1.smb",
        0,
        1103972.4298016021,
        1103968.5297350194,
        1103972.429801602,
        1103968.529735019,
    ),
    (
        "Fillet/vase2.smb",
        0,
        1104218.3523435267,
        1104219.0619762642,
        1104218.3523435264,
        1104219.0619762645,
    ),
    (
        "Fillet/vase3.smb",
        0,
        1103585.4357917241,
        1103584.0778647081,
        1103585.4357917255,
        1103584.0778647095,
    ),
    (
        "Mouse.smb",
        0,
        141.57579582121338,
        141.57785002784453,
        141.57579582121338,
        141.57785002784453,
    ),
    (
        "Platform.smp",
        0,
        84827833.09388979,
        84827833.09389365,
        84827833.0938899,
        84827833.09389386,
    ),
    (
        "Shell/ShellBrep_983.smb",
        0,
        1101.1090658863886,
        1101.1090658863957,
        1101.1090658863886,
        1101.1090658863957,
    ),
    (
        "Shell/problemShell.smb",
        0,
        1494500.2368000003,
        1494500.2367999998,
        1494500.2367999998,
        1494500.2367999994,
    ),
    (
        "Shell/problemShell1.smb",
        0,
        1494500.2368000003,
        1494500.2367999998,
        1494500.2367999998,
        1494500.2367999994,
    ),
    (
        "Shell/problemShell10.smb",
        0,
        604684.0800805318,
        604684.080080532,
        604684.0800805317,
        604684.0800805319,
    ),
    (
        "Shell/problemShell11.smb",
        0,
        868153.2187743338,
        868153.2187733648,
        868153.2187743338,
        868153.2187733648,
    ),
    (
        "Shell/problemShell12.smb",
        0,
        662977.1216174717,
        662977.1216174732,
        662977.1216174718,
        662977.1216174733,
    ),
    (
        "Shell/problemShell13.smb",
        0,
        662977.1216174717,
        662977.1216174732,
        662977.1216174718,
        662977.1216174733,
    ),
    (
        "Shell/problemShell14.smb",
        0,
        1494500.2368000003,
        1494500.2367999998,
        1494500.2367999998,
        1494500.2367999994,
    ),
    (
        "Shell/problemShell15.smb",
        0,
        305595.6444514261,
        305595.6444514248,
        305595.6444514261,
        305595.6444514248,
    ),
    (
        "Shell/problemShell16.smb",
        0,
        458248.7959689218,
        458248.8231987011,
        458248.7959689218,
        458248.8231987011,
    ),
    (
        "Shell/problemShell17.smb",
        0,
        662977.1216174717,
        662977.1216174732,
        662977.1216174718,
        662977.1216174733,
    ),
    (
        "Shell/problemShell2.smb",
        0,
        1494500.2368000008,
        1494500.2367999998,
        1494500.2368000003,
        1494500.2367999994,
    ),
    (
        "Shell/problemShell20.smb",
        0,
        1494500.2368000003,
        1494500.2367999998,
        1494500.2367999998,
        1494500.2367999994,
    ),
    (
        "Shell/problemShell21.smb",
        0,
        1494500.2368000003,
        1494500.2367999998,
        1494500.2367999998,
        1494500.2367999994,
    ),
    (
        "Shell/problemShell22.smb",
        0,
        1369925.5209930176,
        1369925.5209930118,
        1369925.5209930176,
        1369925.5209930118,
    ),
    (
        "Shell/problemShell23.smb",
        0,
        1494500.2368000003,
        1494500.2367999998,
        1494500.2367999998,
        1494500.2367999994,
    ),
    (
        "Shell/problemShell3.smb",
        0,
        190499.61899999972,
        190499.61900000015,
        190499.61899999977,
        190499.6190000002,
    ),
    (
        "Shell/problemShell4.smb",
        0,
        205925.9194055124,
        205925.9194055129,
        205925.91940551248,
        205925.91940551295,
    ),
    (
        "Shell/problemShell5.smb",
        0,
        432251.9085182722,
        432251.9085182715,
        432251.9085182723,
        432251.9085182715,
    ),
    (
        "Shell/problemShell6.smb",
        0,
        415216318.5931697,
        415216318.593149,
        415216318.5931697,
        415216318.593149,
    ),
    (
        "Shell/problemShell7.smb",
        0,
        91499931533.85843,
        91499931533.85844,
        91499931533.85843,
        91499931533.85844,
    ),
    (
        "Shell/problemShell8.smb",
        0,
        868153.2187740896,
        868153.2187731222,
        868153.2187740896,
        868153.2187731223,
    ),
    (
        "Shell/problemShell9.smb",
        0,
        868153.2187740896,
        868153.2187731222,
        868153.2187740896,
        868153.2187731223,
    ),
    (
        "SmallBox.smp",
        0,
        10885999.99859326,
        10885999.998593235,
        10885999.99859325,
        10885999.998593235,
    ),
    (
        "Solids/BooleanBug.smp",
        0,
        94.78150301536384,
        94.78150301537067,
        94.78150301536384,
        94.78150301537067,
    ),
    (
        "Solids/InsetCone.smb",
        0,
        230961.5695020142,
        230961.56950201423,
        230961.56950201423,
        230961.56950201426,
    ),
    (
        "Solids/InsetTaperedExtrude.smb",
        0,
        509413.33748418035,
        509413.42634675803,
        509413.33748418035,
        509413.4263467579,
    ),
    (
        "Solids/OffsetBoxCylOnCorner.smb",
        0,
        871130.9285412554,
        871130.9285412556,
        871130.9285412554,
        871130.9285412556,
    ),
    (
        "Solids/OffsetBoxCylOnCorner_edited.smb",
        0,
        871130.9292066807,
        871130.9292066809,
        871130.9292066807,
        871130.9292066809,
    ),
    (
        "Solids/OffsetBoxCylOnCorner_healed.smb",
        0,
        871130.9292066807,
        871130.9292066809,
        871130.9292066807,
        871130.9292066809,
    ),
    (
        "Solids/OffsetBoxSphOnCorner.smb",
        0,
        1197773.5343428105,
        1197773.5310147386,
        1197773.5343428105,
        1197773.5310147384,
    ),
    (
        "Solids/OffsetBoxWithBoss.smb",
        0,
        492744.86723370804,
        492744.8672337062,
        492744.86723370804,
        492744.8672337062,
    ),
    (
        "Solids/boolean_trees.smp",
        2,
        44.99999999999999,
        45.00000000000001,
        44.99999999999999,
        45.00000000000001,
    ),
    (
        "Solids/boolean_trees.smp",
        5,
        548.609632312034,
        548.5553318534471,
        548.5553138761218,
        548.5553384225661,
    ),
    (
        "Solids/boolean_trees_result.smb",
        0,
        1268.623960658388,
        1268.6372730776845,
        1268.6239606583877,
        1268.6372730776845,
    ),
    (
        "Solids/extrusion.smb",
        0,
        591.8357935230322,
        591.8357935230324,
        591.8357935230322,
        591.8357935230324,
    ),
    (
        "Solids/extrusion2.smb",
        0,
        2278.9438933532665,
        2278.9364537826136,
        2278.9438933532665,
        2278.9364537826136,
    ),
    (
        "Solids/extrusion_old.smb",
        0,
        700.4111951399934,
        700.4463823026618,
        700.4111951399934,
        700.4463823026618,
    ),
    (
        "Solids/nurb_fillet.smb",
        0,
        1252.270253251995,
        1252.3055148975282,
        1252.2702532519947,
        1252.305514897528,
    ),
    (
        "Solids/off_extrusion.smb",
        0,
        591.8357935230322,
        591.8357935230324,
        591.8357935230322,
        591.8357935230324,
    ),
    (
        "Solids/offset_bracket2.smb",
        0,
        5575.097426334338,
        5575.096719189686,
        5575.097426334338,
        5575.096719189686,
    ),
    (
        "Solids/pyramid.smb",
        0,
        983422.1665753095,
        983422.1342764077,
        983422.1665753095,
        983422.1342764077,
    ),
    (
        "Solids/revolution.smb",
        0,
        1062.1899957658777,
        1062.2203209358545,
        1062.1899957658777,
        1062.2203209358545,
    ),
    (
        "Solids/revolution2.smb",
        0,
        3122.737278450887,
        3122.795318224679,
        3122.7372784508866,
        3122.7953182246783,
    ),
    (
        "Solids/revolution_360.smb",
        0,
        31783.713337898615,
        31783.71281891036,
        31783.713337898622,
        31783.712818910364,
    ),
    (
        "Solids/revolution_mold.smb",
        0,
        100912.97629248399,
        100913.14347117723,
        100912.97629248399,
        100913.14347117723,
    ),
    (
        "Tess/nurby_box_cyl_sphere.smb",
        0,
        2831.4268527663867,
        2831.4237262621405,
        2831.426852766387,
        2831.423726262141,
    ),
    (
        "box_with_large_fillet.smb",
        0,
        862.6548245809618,
        862.6548245809633,
        862.6548245809618,
        862.6548245809633,
    ),
    (
        "fixtures/offset.offset_executive_p2/data/issue1528_box_cylinder.smb",
        0,
        26373.612905720667,
        26373.612869703607,
        26373.612905720667,
        26373.612869703607,
    ),
    (
        "fixtures/offset.offset_executive_p2/data/issue1528_plate.smb",
        0,
        26373.612906841216,
        26373.612869703364,
        26373.612906841216,
        26373.612869703364,
    ),
    (
        "ssi_rev_rev_sing.smp",
        0,
        1496.5082567780776,
        1496.508256778078,
        1496.5082567780782,
        1496.5082567780787,
    ),
];

/// `corpus.parity`'s accuracy.
const CORPUS_ACCURACY: f64 = 1e-6;

/// A distinct identity per topology object of B-rep `brep`, as `corpus.parity` gives them.
fn key_of(brep: usize, e: EntityRef) -> ObjectRef {
    let (type_code, kind, i) = match e {
        EntityRef::Brep(x) => (16011, 0, x.index()),
        EntityRef::Region(x) => (16012, 1, x.index()),
        EntityRef::Shell(x) => (16013, 2, x.index()),
        EntityRef::Faceuse(x) => (16022, 3, x.index()),
        EntityRef::Face(x) => (16002, 4, x.index()),
        EntityRef::Loopuse(x) => (16004, 5, x.index()),
        EntityRef::Loop(x) => (16003, 6, x.index()),
        EntityRef::Edgeuse(x) => (16005, 7, x.index()),
        EntityRef::Edge(x) => (16016, 8, x.index()),
        EntityRef::Vertexuse(x) => (16006, 9, x.index()),
        EntityRef::Vertex(x) => (16021, 10, x.index()),
    };
    let brep = (brep as u64 + 1) << 36;
    ObjectRef {
        type_code,
        key: ObjectKey(brep | ((kind + 1) << 32) | u64::from(i)),
    }
}

/// A model file's B-reps as `corpus.parity` reads them, with the readers' records.
fn read_breps(k: &mut Kernel, bytes: Vec<u8>) -> (Vec<Brep>, AttributeStore, NotifyRecords) {
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    if bytes.starts_with(b"//[Output Summary]") {
        let mut env = PartEnv {
            attributes: &mut attributes,
            key_of: &key_of,
            records: &mut records,
            next_key: 1 << 62,
        };
        let breps = read_part_from_file(k, bytes, FileType::Ascii, &mut env)
            .unwrap()
            .breps;
        return (breps, attributes, records);
    }
    let mut b = Brep::new_with_infinite_region().unwrap();
    let one = |e| key_of(0, e);
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &one,
        records: &mut records,
        next_key: 1 << 62,
    };
    read_from_file(k, &mut b, bytes, FileType::Ascii, false, &mut env).unwrap();
    (vec![b], attributes, records)
}

/// B-rep `i` of a read file written as ASCII (with its NURBs) and read back.
fn round_trip(
    k: &mut Kernel,
    breps: &[Brep],
    attributes: &AttributeStore,
    records: &NotifyRecords,
    i: usize,
) -> Brep {
    let key = |e| key_of(i, e);
    let env = FromBrepEnv {
        attributes,
        key_of: &key,
        records,
        curve_key: &|_: EdgeId| ObjectKey(0),
        uv_curve_key: &|_: EdgeuseId| ObjectKey(0),
        surface_key: &|_: FaceId| ObjectKey(0),
    };
    let mut all = Vec::new();
    let bytes = BrepData::from_brep(&breps[i], &env, &mut all)
        .and_then(|mut data| data.write_to_file_with_nurbs(k, FileType::Ascii))
        .unwrap();
    read_breps(k, bytes).0.remove(0)
}

/// #16735: every `corpus.parity` volume the closure moved, as read and round-tripped,
/// passes the three checks (module doc of `support/volume_leak.rs`). Heavy (116 files,
/// each measured at four origins and its trims sampled): run with
/// `cargo test --profile goldens -p solidcore --test issue_16735_volume_moves_are_the_old_leak -- --ignored`
/// from `solidcore/` with `upstream/` cloned (the goldens' own requirement).
#[test]
#[ignore = "heavy: 116 corpus files; needs upstream/TestFiles"]
fn corpus_parity_moves_are_the_old_leak() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut failed = Vec::new();
    let mut passed = 0;
    for (file, i, old, new, old_rt, new_rt) in CORPUS_ROWS {
        let upstream = root.join("upstream/TestFiles/pt_TestFiles").join(file);
        let path = if upstream.exists() {
            upstream
        } else {
            root.join(file)
        };
        let mut k = Kernel::default();
        let (mut breps, attributes, records) = read_breps(&mut k, std::fs::read(&path).unwrap());
        assert!(
            is_manifold_solid(&breps[i]).unwrap(),
            "{file}[{i}]: not a solid"
        );
        let mut back = round_trip(&mut k, &breps, &attributes, &records, i);
        for (side, b, old, new) in [
            ("read", &mut breps[i], old, new),
            ("round trip", &mut back, old_rt, new_rt),
        ] {
            match volume_leak::moved_by_the_old_leak(&k, b, old, new, CORPUS_ACCURACY) {
                Ok(line) => {
                    passed += 1;
                    eprintln!("PASS {file}[{i}] {side}: {line}");
                }
                Err(line) => {
                    eprintln!("FAIL {file}[{i}] {side}: {line}");
                    failed.push(format!("{file}[{i}] {side}: {line}"));
                }
            }
        }
    }
    eprintln!("{passed} passed, {} failed", failed.len());
    assert!(failed.is_empty(), "{failed:#?}");
}
