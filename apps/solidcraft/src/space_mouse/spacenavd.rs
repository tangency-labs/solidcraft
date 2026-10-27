//! Reading a 3D mouse through `spacenavd` on Linux. The daemon owns the device and applies what
//! `spnavcfg` sets (swapped and inverted axes, sensitivity, dead zones) before it hands the motion
//! to its clients, so a program that reads the HID device itself bypasses that setup. Over its
//! socket the daemon sends fixed 32-byte records: eight native-endian `i32`, the first the kind of
//! record (0 for motion, then x y z rx ry rz and the milliseconds since the last record).
//! Button records (1 pressed, 2 released, then the button's number from 0) are read too; other
//! records (from newer daemons also configuration changes) are ignored.
//!
//! The socket is plain file I/O, so no library is needed, and it needs no udev rule: the daemon
//! is what has access to the device.

use std::io::Read;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

use solidcraft_ui_egui::space_mouse::{Axes, FULL_SCALE};

/// Where the daemon listens. `/var/run` is a link to `/run` on current systems.
const SOCKETS: [&str; 2] = ["/var/run/spnav.sock", "/run/spnav.sock"];

const RECORD_BYTES: usize = 32;

/// What the daemon reports at full deflection (seen with a SpaceMouse Compact and a default
/// sensitivity). Our axes are in the raw HID units of [`FULL_SCALE`].
const DAEMON_FULL_SCALE: i64 = 500;

/// Whether the daemon's socket is there, without connecting to it.
pub fn exists() -> bool {
    SOCKETS.iter().any(|path| Path::new(path).exists())
}

pub fn connect() -> Option<UnixStream> {
    SOCKETS.iter().find_map(|path| connect_to(Path::new(path)))
}

fn connect_to(path: &Path) -> Option<UnixStream> {
    UnixStream::connect(path).ok()
}

/// The axes of one record, or `None` for a record that is not motion, in the device's raw frame
/// (3Dconnexion's, as calibration has it: x right, y towards the user, z down, the rotations
/// about them right-handed).
///
/// The daemon's translations have x right, y away from the user and z up, so y and z change
/// sign. Its rotations don't follow them (#2365): rx is the top tilting away, ry the twist
/// anticlockwise seen from above (the rotation about its up axis) and rz the tilt left (about
/// the axis away from the user), so rx changes sign, its ry is our -Rz and its rz our Ry. With
/// nothing set in `spnavcfg` the reading is then what the device itself sends, and the driver
/// mapping treats it as it does a hidraw one; what the user swaps or inverts there carries over.
fn decode(record: &[u8; RECORD_BYTES]) -> Option<Axes> {
    let mut data = [0_i32; 8];
    for (slot, bytes) in data.iter_mut().zip(record.as_chunks::<4>().0) {
        *slot = i32::from_ne_bytes(*bytes);
    }
    let [kind, x, y, z, rx, ry, rz, _period] = data;
    (kind == 0).then(|| Axes {
        translation: [unit(x), unit(-i64::from(y)), unit(-i64::from(z))],
        rotation: [unit(-i64::from(rx)), unit(rz), unit(-i64::from(ry))],
    })
}

/// The button a button record is about, from 0, and whether it went down; `None` for any other
/// record, or a button past the 32 a bitmask holds.
fn button(record: &[u8; RECORD_BYTES]) -> Option<(u32, bool)> {
    let mut data = [0_i32; 2];
    for (slot, bytes) in data.iter_mut().zip(record.as_chunks::<4>().0) {
        *slot = i32::from_ne_bytes(*bytes);
    }
    let [kind, number] = data;
    let number = u32::try_from(number).ok().filter(|&n| n < 32)?;
    match kind {
        1 => Some((number, true)),
        2 => Some((number, false)),
        _ => None,
    }
}

/// A daemon value in our raw units. Out-of-range values stay in `i16`: they are input from another
/// process.
fn unit(value: impl Into<i64>) -> i16 {
    let scaled = value.into() * i64::from(FULL_SCALE) / DAEMON_FULL_SCALE;
    i16::try_from(scaled.clamp(i64::from(i16::MIN), i64::from(i16::MAX))).unwrap_or_default()
}

/// Hands every motion record to `publish` and every button record to `pressed` until the daemon
/// closes the connection or fails, or `stop` says so. A stream with a read timeout
/// (`set_read_timeout`) times out when the daemon is quiet, which is no record and no disconnect:
/// `stop` is asked, and the read goes on, with what of a record had arrived kept. `stop` is also asked
/// at least every `check_every` while records keep arriving, so a daemon that never pauses for that
/// long (a moving cap sends every few milliseconds) does not keep the reader from noticing that the
/// device it prefers is back. A partial record at the end is dropped.
pub fn read(
    mut stream: impl Read,
    mut publish: impl FnMut(Axes),
    mut pressed: impl FnMut(u32, bool),
    mut stop: impl FnMut() -> bool,
    check_every: Duration,
) {
    let mut record = [0_u8; RECORD_BYTES];
    let mut filled = 0;
    let mut asked = Instant::now();
    loop {
        if asked.elapsed() >= check_every {
            asked = Instant::now();
            if stop() {
                return;
            }
        }
        match stream.read(record.get_mut(filled..).unwrap_or_default()) {
            Ok(0) => {
                log::info!("3D mouse: spacenavd connection ended");
                return;
            }
            Ok(size) => {
                filled += size;
                if filled == RECORD_BYTES {
                    filled = 0;
                    if let Some(axes) = decode(&record) {
                        publish(axes);
                    } else if let Some((number, down)) = button(&record) {
                        pressed(number, down);
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                asked = Instant::now();
                if stop() {
                    return;
                }
            }
            Err(error) => {
                log::info!("3D mouse: spacenavd connection ended: {error}");
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn record(data: [i32; 8]) -> [u8; RECORD_BYTES] {
        let mut bytes = [0_u8; RECORD_BYTES];
        for (chunk, value) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(data) {
            *chunk = value.to_ne_bytes();
        }
        bytes
    }

    #[test]
    fn motion_records_become_axes_in_the_devices_raw_frame() {
        // Full deflection of the daemon's x, y and z is the device's full scale, and y and z flip.
        assert_eq!(decode(&record([0, 500, 500, 500, 0, 0, 0, 8])), Some(Axes { translation: [350, -350, -350], rotation: [0; 3] }));
        // Rotations: rx flips, the daemon's ry is the twist (our Rz, flipped) and its rz the tilt left (our Ry).
        assert_eq!(decode(&record([0, 0, 0, 0, 250, 0, 0, 8])), Some(Axes { translation: [0; 3], rotation: [-175, 0, 0] }));
        assert_eq!(decode(&record([0, 0, 0, 0, 0, 250, 0, 8])), Some(Axes { translation: [0; 3], rotation: [0, 0, -175] }));
        assert_eq!(decode(&record([0, 0, 0, 0, 0, 0, 250, 8])), Some(Axes { translation: [0; 3], rotation: [0, 175, 0] }));
        // Rest is a motion record of zeros.
        assert_eq!(decode(&record([0, 0, 0, 0, 0, 0, 0, 1200])), Some(Axes::default()));
    }

    /// With nothing set in `spnavcfg`, each motion of the cap moves the view as it does read
    /// through hidraw (#2365: tilt left/right and twist came out swapped). The daemon's record for
    /// each motion one way at full deflection, and the raw HID reading the device sends for it.
    #[test]
    fn each_motion_through_the_daemon_moves_the_view_as_through_hidraw() {
        use solidcraft_ui_egui::space_mouse::DriverMapping;
        let motions: [(&str, [i32; 6], [i16; 6], [f64; 6]); 6] = [
            ("slide right", [500, 0, 0, 0, 0, 0], [350, 0, 0, 0, 0, 0], [1.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
            ("push away", [0, 500, 0, 0, 0, 0], [0, -350, 0, 0, 0, 0], [0.0, -1.0, 0.0, 0.0, 0.0, 0.0]),
            ("lift", [0, 0, 500, 0, 0, 0], [0, 0, -350, 0, 0, 0], [0.0, 0.0, -1.0, 0.0, 0.0, 0.0]),
            // Logical rotations: orbit up/down, orbit sideways, roll (`navigation::device_camera`).
            ("tilt the top away", [0, 0, 0, 500, 0, 0], [0, 0, 0, -350, 0, 0], [0.0, 0.0, 0.0, -1.0, 0.0, 0.0]),
            ("twist anticlockwise from above", [0, 0, 0, 0, 500, 0], [0, 0, 0, 0, 0, -350], [0.0, 0.0, 0.0, 0.0, 1.0, 0.0]),
            ("tilt left", [0, 0, 0, 0, 0, 500], [0, 0, 0, 0, 350, 0], [0.0, 0.0, 0.0, 0.0, 0.0, -1.0]),
        ];
        let view = |axes: Axes| {
            let motion = DriverMapping::object_mode().apply(axes);
            [motion.translation, motion.rotation].concat()
        };
        for (name, [x, y, z, rx, ry, rz], [hx, hy, hz, hrx, hry, hrz], expected) in motions {
            let axes = decode(&record([0, x, y, z, rx, ry, rz, 8]));
            let hid = Axes { translation: [hx, hy, hz], rotation: [hrx, hry, hrz] };
            assert_eq!(axes, Some(hid), "{name}: the device's own reading");
            assert_eq!(axes.map(view), Some(expected.to_vec()), "{name}: the view");
            // And the other way, the opposite motion.
            let back = decode(&record([0, -x, -y, -z, -rx, -ry, -rz, 8])).map(view);
            assert_eq!(back, Some(expected.map(|v| -v).to_vec()), "{name}, the other way");
        }
    }

    #[test]
    fn what_spnavcfg_swaps_reaches_the_view() {
        // The daemon sends a push away as y. With Y and Z swapped in spnavcfg it sends the same push
        // as z, and the view gets a lift (raw z) instead of the zoom (raw y).
        let before = decode(&record([0, 0, 500, 0, 0, 0, 0, 8])).map(|axes| axes.translation);
        let after = decode(&record([0, 0, 0, 500, 0, 0, 0, 8])).map(|axes| axes.translation);
        assert_eq!(before, Some([0, -350, 0]));
        assert_eq!(after, Some([0, 0, -350]));
    }

    #[test]
    fn other_records_and_hostile_values_are_harmless() {
        // A button record (kind 1, press) and one of an unknown kind are not motion.
        assert_eq!(decode(&record([1, 0, 1, 0, 0, 0, 0, 0])), None);
        // Nor are hostile button numbers a button.
        assert_eq!(button(&record([1, -1, 0, 0, 0, 0, 0, 0])), None);
        assert_eq!(button(&record([1, i32::MAX, 0, 0, 0, 0, 0, 0])), None);
        assert_eq!(button(&record([7, 3, 0, 0, 0, 0, 0, 0])), None);
        assert_eq!(decode(&record([7, 500, 500, 500, 500, 500, 500, 0])), None);
        // Nothing overflows on the extremes of `i32`.
        let extreme = decode(&record([0, i32::MIN, i32::MIN, i32::MAX, i32::MAX, i32::MIN, i32::MIN, -1]));
        assert_eq!(extreme.map(|axes| axes.translation[0]), Some(i16::MIN));
        assert_eq!(extreme.map(|axes| axes.translation[1]), Some(i16::MAX));
        assert_eq!(extreme.map(|axes| axes.rotation[0]), Some(i16::MIN));
        assert_eq!(extreme.map(|axes| axes.rotation[1]), Some(i16::MIN));
        assert_eq!(extreme.map(|axes| axes.rotation[2]), Some(i16::MAX));
    }

    #[test]
    fn a_stream_is_read_record_by_record_until_it_ends() {
        let (mut daemon, client) = UnixStream::pair().expect("a socket pair");
        let mut bytes = Vec::new();
        bytes.extend(record([0, 100, 0, 0, 0, 0, 0, 8]));
        bytes.extend(record([1, 4, 0, 0, 0, 0, 0, 0])); // button 4 down
        bytes.extend(record([2, 4, 0, 0, 0, 0, 0, 0])); // and up
        bytes.extend(record([1, 40, 0, 0, 0, 0, 0, 0])); // past the 32 a bitmask holds: skipped
        bytes.extend(record([0, 0, 0, 0, 0, 0, 0, 900]));
        bytes.extend([1, 2, 3]); // the daemon went away mid-record
        daemon.write_all(&bytes).expect("written");
        drop(daemon);
        let (mut seen, mut buttons) = (Vec::new(), Vec::new());
        read(client, |axes| seen.push(axes.translation), |number, down| buttons.push((number, down)), || false, Duration::from_secs(3600));
        assert_eq!(seen, [[70, 0, 0], [0, 0, 0]]);
        assert_eq!(buttons, [(4, true), (4, false)]);
    }

    #[test]
    fn a_quiet_daemon_times_out_without_losing_the_record_it_was_sending() {
        use crate::space_mouse::fake::Script;
        use std::io::ErrorKind::{TimedOut, WouldBlock};
        let bytes = record([0, 100, 0, 0, 0, 0, 0, 8]);
        let mut script = Script::new();
        script.timeout(WouldBlock).data(&bytes[..10]).timeout(TimedOut).timeout(WouldBlock).data(&bytes[10..]).timeout(WouldBlock).end();
        let (mut seen, mut asked) = (Vec::new(), 0);
        read(
            script,
            |axes| seen.push(axes.translation),
            |_, _| {},
            || {
                asked += 1;
                false
            },
            Duration::from_secs(3600),
        );
        assert_eq!(seen, [[70, 0, 0]]);
        assert_eq!(asked, 4, "each timeout asks whether to stop");
    }

    #[test]
    fn a_stop_ends_the_read_at_the_next_timeout_only() {
        use crate::space_mouse::fake::Script;
        let bytes = record([0, 100, 0, 0, 0, 0, 0, 8]);
        let mut script = Script::new();
        script.data(&bytes).timeout(std::io::ErrorKind::WouldBlock).data(&bytes);
        let mut seen = 0;
        read(script, |_| seen += 1, |_, _| {}, || true, Duration::from_secs(3600));
        assert_eq!(seen, 1, "the record before the timeout arrived; the one after never was read");
    }

    /// A daemon that never goes quiet (a moving cap sends every few milliseconds) is still asked whether to
    /// stop, at the interval given, so the reader can go back to a device that returned.
    #[test]
    fn a_busy_daemon_is_asked_whether_to_stop_on_a_timer() {
        // Ends by itself after half a second, so a read that never asks fails the assertion below
        // instead of hanging the test.
        struct Busy(Instant);
        impl Read for Busy {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                if self.0.elapsed() > Duration::from_millis(500) {
                    return Ok(0);
                }
                std::thread::sleep(Duration::from_millis(2));
                let bytes = record([0, 100, 0, 0, 0, 0, 0, 8]);
                let size = bytes.len().min(buf.len());
                buf[..size].copy_from_slice(&bytes[..size]);
                Ok(size)
            }
        }
        let (mut seen, mut asked) = (0, 0);
        read(
            Busy(Instant::now()),
            |_| seen += 1,
            |_, _| {},
            || {
                asked += 1;
                asked == 3
            },
            Duration::from_millis(10),
        );
        assert_eq!(asked, 3, "asked on the timer although no read ever timed out");
        assert!(seen > 3, "records kept coming meanwhile: {seen}");
    }

    #[test]
    fn no_daemon_means_no_connection() {
        let missing = std::env::temp_dir().join(format!("sc-no-spnav-{}.sock", std::process::id()));
        assert!(connect_to(&missing).is_none());
    }
}
