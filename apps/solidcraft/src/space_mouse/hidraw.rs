//! Finding a 3D mouse on Linux. The kernel exposes every HID interface as `/dev/hidrawN`, with
//! its report descriptor and ids under `/sys/class/hidraw/hidrawN/device/`. Reading them is plain
//! file I/O, so no udev or hidapi library is needed.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use solidcraft_space_mouse::{Descriptor, Layout};

const SYSFS: &str = "/sys/class/hidraw";
const DEV: &str = "/dev";

/// `O_NONBLOCK` on Linux's common architectures (x86, ARM, RISC-V). The device is opened with it so
/// a read with nothing to read returns at once and the reader can see whether it should stop.
pub const O_NONBLOCK: i32 = 0o4000;

/// Opens a hidraw node the way the Direct reader reads it: read-only and non-blocking.
pub fn open(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().read(true).custom_flags(O_NONBLOCK).open(path)
}

/// How long the reader waits when the device has nothing to say, which is also the most a report
/// can be delayed.
const IDLE_POLL: Duration = Duration::from_millis(5);

pub struct Device {
    pub path: PathBuf,
    pub layout: Layout,
    /// Its vendor and product ids, when the kernel gives them.
    pub ids: Option<(u16, u16)>,
}

/// The first hidraw interface that is a multi-axis controller with motion axes, whoever made it.
pub fn find() -> Option<Device> {
    let mut names: Vec<_> = std::fs::read_dir(SYSFS).ok()?.filter_map(Result::ok).map(|entry| entry.file_name()).collect();
    names.sort();
    names.into_iter().find_map(|name| probe(&Path::new(SYSFS).join(&name), Path::new(DEV).join(&name)))
}

/// Hands every report to `publish` until the device is gone (end of file or an error) or `stop`
/// says so. `source` is non-blocking: when it has nothing, `stop` is asked and the reader waits
/// [`IDLE_POLL`]. Reports are read and handed over exactly as they were with blocking reads.
pub fn read_reports(mut source: impl Read, mut publish: impl FnMut(&[u8]), mut stop: impl FnMut() -> bool) {
    let mut report = [0_u8; super::REPORT_BUFFER_BYTES];
    loop {
        match source.read(&mut report) {
            Ok(0) => return,
            Ok(size) => publish(report.get(..size).unwrap_or_default()),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if stop() {
                    return;
                }
                std::thread::sleep(IDLE_POLL);
            }
            Err(error) => {
                log::info!("3D mouse disconnected: {error}");
                return;
            }
        }
    }
}

fn probe(sysfs: &Path, path: PathBuf) -> Option<Device> {
    let descriptor = Descriptor::parse(&std::fs::read(sysfs.join("device/report_descriptor")).ok()?);
    // A controller that declares no motion axis (buttons only, say) isn't read.
    let layout = descriptor.is_multi_axis_controller().then(|| descriptor.layout()).flatten()?;
    let ids = std::fs::read_to_string(sysfs.join("device/uevent")).ok().as_deref().and_then(ids);
    Some(Device { path, layout, ids })
}

/// The vendor and product ids in a HID device's uevent: `HID_ID=bus:vendor:product`, in hex.
fn ids(uevent: &str) -> Option<(u16, u16)> {
    let id = uevent.lines().find_map(|line| line.strip_prefix("HID_ID="))?;
    let mut hex = id.split(':').skip(1).map(|part| u32::from_str_radix(part, 16).ok().and_then(|v| u16::try_from(v).ok()));
    Some((hex.next()??, hex.next()??))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real EACCES, as a user without the udev rule gets: a file of the user's own with no permission
    /// bits. Skipped for root, which can open anything.
    #[test]
    fn a_device_without_permission_is_denied_and_the_daemon_takes_over_within_the_backoff() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if std::fs::metadata("/proc/self").is_ok_and(|meta| meta.uid() == 0) {
            return;
        }
        let dir = std::env::temp_dir().join(format!("sc-hidraw-denied-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let node = dir.join("hidraw0");
        std::fs::write(&node, [0_u8; 4]).unwrap();
        std::fs::set_permissions(&node, std::fs::Permissions::from_mode(0o000)).unwrap();
        let denied = open(&node);
        std::fs::set_permissions(&node, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(open(&node).is_ok(), "the same node opens once it is readable");
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(denied.err().map(|error| error.kind()), Some(std::io::ErrorKind::PermissionDenied));
        // The failure is the choice's `since_failed`, with an injected clock: inside the backoff the
        // daemon is read, whichever driver was asked for; after it the device is tried again.
        use super::super::{Pick, SpaceMouseDriver, UNREADABLE_RETRY, choose, direct_usable};
        for wanted in [SpaceMouseDriver::Automatic, SpaceMouseDriver::Spacenavd, SpaceMouseDriver::Direct] {
            for ago in [Duration::ZERO, Duration::from_secs(29)] {
                assert_eq!(choose(wanted, true, direct_usable(true, Some(ago), UNREADABLE_RETRY)), Some(Pick::Spacenavd), "{wanted:?} {ago:?}");
            }
        }
        assert_eq!(choose(SpaceMouseDriver::Direct, true, direct_usable(true, Some(UNREADABLE_RETRY), UNREADABLE_RETRY)), Some(Pick::Direct));
        // And the daemon's own reader delivers over a fake connection.
        let (mut daemon, client) = std::os::unix::net::UnixStream::pair().unwrap();
        let mut record = [0_u8; 32];
        record[4..8].copy_from_slice(&100_i32.to_ne_bytes());
        std::io::Write::write_all(&mut daemon, &record).unwrap();
        drop(daemon);
        let mut seen = 0;
        super::super::spacenavd::read(client, |_| seen += 1, |_, _| {}, || false, Duration::from_secs(3600));
        assert_eq!(seen, 1);
    }

    #[test]
    fn reports_come_through_idle_gaps_until_the_device_ends_or_a_stop() {
        use crate::space_mouse::fake::Script;
        let mut script = Script::new();
        script.timeout(std::io::ErrorKind::WouldBlock).data(&[1, 2, 3]).timeout(std::io::ErrorKind::WouldBlock).data(&[4]).end();
        let (mut seen, mut asked) = (Vec::new(), 0);
        read_reports(
            script,
            |report| seen.push(report.to_vec()),
            || {
                asked += 1;
                false
            },
        );
        assert_eq!(seen, [vec![1, 2, 3], vec![4]]);
        assert_eq!(asked, 2);
        let mut script = Script::new();
        script.data(&[1]).timeout(std::io::ErrorKind::WouldBlock).data(&[2]);
        let mut seen = Vec::new();
        read_reports(script, |report| seen.push(report.to_vec()), || true);
        assert_eq!(seen, [vec![1]], "a stop at an idle moment ends the read");
        // An unplugged device is an error, which ends the read as before.
        let mut script = Script::new();
        script.data(&[1]).fail(std::io::ErrorKind::BrokenPipe).data(&[2]);
        let mut seen = Vec::new();
        read_reports(script, |report| seen.push(report.to_vec()), || false);
        assert_eq!(seen, [vec![1]]);
    }

    #[test]
    fn ids_come_from_the_uevent() {
        let wireless = "DRIVER=hid-generic\nHID_ID=0005:0000256F:0000C63A\nHID_NAME=SpaceMouse Wireless BT\n";
        assert_eq!(ids(wireless), Some((0x256f, 0xc63a)));
        assert_eq!(ids("HID_ID=0003:0000046D\n"), None);
        assert_eq!(ids("HID_ID=0003:0001046D:0000C62B\n"), None, "a vendor past 16 bits isn't a USB id");
        assert_eq!(ids("HID_NAME=keyboard\n"), None);
    }
}
