//! Read-only device-tree selection. Reuse FlexInput's CfgMgr32 helpers; do not
//! infer physical identity from a controller's name or VID/PID.
use super::policy::{contains, push_unique, valid_physical_id, virtual_id};
use anyhow::{bail, ensure, Result};
use flexinput_devices::devnode;
use windows_sys::{
    core::GUID,
    Win32::Devices::Properties::{DEVPKEY_Device_CompatibleIds, DEVPKEY_Device_HardwareIds},
};

#[derive(Clone, Debug)]
pub(super) struct Node {
    pub id: String,
    pub parents: Vec<String>,
    pub gaming: bool,
    pub other_input: bool,
    pub xinput: bool,
    pub known: bool,
}

fn hid_roles(id: &str, hardware: &[String], compatible: &[String]) -> (bool, bool, bool) {
    // HID usage and system-role IDs commonly live in HardwareIds; an empty
    // CompatibleIds list is normal for a physical HID collection.
    let identifiers = hardware
        .iter()
        .chain(compatible)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_uppercase();
    let gaming = [
        "HID_DEVICE_SYSTEM_GAME",
        "HID_DEVICE_SYSTEM_JOYSTICK",
        "HID_DEVICE_UP:0001_U:0004",
        "HID_DEVICE_UP:0001_U:0005",
    ]
    .iter()
    .any(|role| identifiers.contains(role))
        || id.to_ascii_uppercase().contains("&IG_");
    let other_input = [
        "HID_DEVICE_SYSTEM_KEYBOARD",
        "HID_DEVICE_SYSTEM_MOUSE",
        "HID_DEVICE_UP:0001_U:0002",
        "HID_DEVICE_UP:0001_U:0006",
    ]
    .iter()
    .any(|role| identifiers.contains(role));
    let known = gaming || other_input || identifiers.contains("HID_DEVICE_UP:");
    (gaming, other_input, known)
}
impl Node {
    fn is_virtual(&self) -> bool {
        virtual_id(&self.id) || self.parents.iter().any(|p| virtual_id(p))
    }
    fn family(&self) -> Option<&str> {
        let chain: Vec<_> = std::iter::once(&self.id).chain(&self.parents).collect();
        if chain
            .iter()
            .any(|p| p.to_ascii_uppercase().starts_with("BTH"))
        {
            return chain
                .into_iter()
                .find(|p| {
                    let up = p.to_ascii_uppercase();
                    up.starts_with("BTHENUM\\DEV_")
                        || up.starts_with("BTHLE\\DEV_")
                        || up.starts_with("BTHLEDEVICE\\")
                })
                .map(String::as_str);
        }
        chain
            .into_iter()
            .find(|p| {
                let up = p.to_ascii_uppercase();
                up.starts_with("USB\\VID_") && !up.contains("&MI_") && !up.contains("&IG_")
            })
            .map(String::as_str)
    }
    fn is_below(&self, id: &str) -> bool {
        self.id.eq_ignore_ascii_case(id) || contains(&self.parents, id)
    }
}

pub(super) fn inventory() -> Vec<Node> {
    // GUID_DEVINTERFACE_HID, not the HID setup class GUID.
    const HID: GUID = GUID::from_u128(0x4d1e55b2_f16f_11cf_88cb_001111000030);
    let mut result = Vec::new();
    for path in devnode::interface_list(&HID) {
        let Some(id) = devnode::interface_instance_id(&path) else {
            continue;
        };
        if result.iter().any(|n: &Node| n.id.eq_ignore_ascii_case(&id)) {
            continue;
        }
        let Some(inst) = devnode::locate(&id) else {
            continue;
        };
        let hardware = devnode::strings_property(inst, &DEVPKEY_Device_HardwareIds);
        let compatible = devnode::strings_property(inst, &DEVPKEY_Device_CompatibleIds);
        let (gaming, other_input, known) = hid_roles(&id, &hardware, &compatible);
        result.push(Node {
            parents: devnode::ancestors(&id, 16),
            id,
            gaming,
            other_input,
            xinput: false,
            known,
        });
    }
    for device in devnode::xinput_devnodes() {
        if let Some(node) = result
            .iter_mut()
            .find(|n| n.id.eq_ignore_ascii_case(&device.instance_id))
        {
            node.xinput = true;
            node.gaming = true;
        } else {
            result.push(Node {
                parents: devnode::ancestors(&device.instance_id, 16),
                id: device.instance_id,
                gaming: true,
                other_input: false,
                xinput: true,
                known: true,
            });
        }
    }
    result
}

pub(super) fn selected(path: &str) -> Result<Option<Vec<String>>> {
    ensure!(
        path.len() <= 2048 && !path.contains('\0') && path.starts_with(r"\\?\"),
        "所选接口没有可定位的 Windows 设备路径；请选择 HID/接管接口，或停止后关闭自动隐藏"
    );
    let Some(id) = devnode::interface_instance_id(path) else {
        return Ok(None);
    };
    plan(&id, &inventory())
}

fn plan(id: &str, all: &[Node]) -> Result<Option<Vec<String>>> {
    let Some(selected) = all.iter().find(|n| n.id.eq_ignore_ascii_case(id)) else {
        return Ok(None);
    };
    ensure!(!selected.is_virtual(), "虚拟手柄不能作为自动隐藏目标");
    ensure!(
        valid_physical_id(&selected.id),
        "该设备实例不支持自动隐藏，请选择可定位的实体 HID/XInput 接口"
    );
    ensure!(
        !selected.other_input && selected.known,
        "该接口不是可确认的实体手柄接口，不能自动隐藏键盘或鼠标"
    );
    let family: Vec<_> = all
        .iter()
        .filter(|n| {
            !n.is_virtual()
                && selected
                    .family()
                    .is_some_and(|base| n.family().is_some_and(|b| b.eq_ignore_ascii_case(base)))
        })
        .collect();
    let xinput: Vec<_> = family.iter().copied().filter(|n| n.xinput).collect();
    ensure!(
        xinput.len() <= 1,
        "同一接收器存在多个 XInput 手柄，无法可靠确定伴随接口；请使用更明确的接口或手动隐藏"
    );
    let gamepads: Vec<_> = family
        .iter()
        .copied()
        .filter(|n| n.gaming && !n.xinput)
        .collect();
    ensure!(
        selected.gaming || !gamepads.is_empty() || !xinput.is_empty(),
        "该接口没有可确认的实体游戏手柄关联"
    );
    if xinput.is_empty() && gamepads.len() > 1 {
        bail!("同一接收器存在多个游戏接口，无法安全地自动隐藏整个设备");
    }
    let mut paths = vec![selected.id.clone()];
    for node in family.iter().filter(|n| n.gaming || n.xinput) {
        push_unique(&mut paths, node.id.clone());
    }
    if let Some(base) = selected
        .family()
        .filter(|b| b.to_ascii_uppercase().starts_with("USB\\VID_"))
    {
        if let Some(xusb) = xinput.first() {
            // XInput bypasses the HID child and opens its XUSB interface. On a
            // composite pad that interface is its own USB function (e.g. MI_00),
            // not the common USB parent that also owns keyboard/mouse siblings.
            let function = xusb.id.to_ascii_uppercase().starts_with("USB\\")
                && !xusb.id.eq_ignore_ascii_case(base);
            let target = if function { xusb.id.as_str() } else { base };
            ensure!(family.iter().filter(|n| n.is_below(target)).all(|n| !n.other_input && n.known), "接收器共享键鼠或未知接口，自动隐藏 USB 基础节点可能影响其他输入；请手动配置隐藏规则");
            push_unique(&mut paths, target.to_string());
        }
    }
    ensure!(
        paths.len() <= 8 && paths.iter().all(|p| valid_physical_id(p)),
        "实体手柄关联不明确，未自动隐藏"
    );
    Ok(Some(paths))
}

pub(super) fn virtual_ids(nodes: &[Node]) -> Vec<String> {
    nodes
        .iter()
        .filter(|n| n.is_virtual())
        .map(|n| n.id.clone())
        .collect()
}
pub(super) fn verify(
    nodes: &[Node],
    hidden: &[String],
    protected: &[String],
    baseline: &[String],
    expect_gamepad: bool,
    require_present: bool,
) -> Result<()> {
    let owned: Vec<_> = nodes
        .iter()
        .filter(|n| {
            n.is_virtual()
                && if protected.is_empty() {
                    !contains(baseline, &n.id)
                } else {
                    protected.iter().any(|p| n.is_below(p))
                }
        })
        .collect();
    if require_present {
        ensure!(
            protected
                .iter()
                .all(|p| owned.iter().any(|n| n.is_below(p))),
            "虚拟输出设备尚未就绪，已暂停输出"
        );
        ensure!(
            !expect_gamepad || owned.iter().any(|n| n.gaming || n.xinput),
            "未能确认虚拟手柄已枚举，已暂停输出"
        );
    }
    for node in owned {
        ensure!(
            !contains(hidden, &node.id) && !node.parents.iter().any(|p| contains(hidden, p)),
            "虚拟输出手柄被现有 HidHide 规则隐藏，请在官方工具恢复该虚拟设备可见后重试"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn node(id: &str, parents: &[&str], gaming: bool, xinput: bool, keyboard: bool) -> Node {
        Node {
            id: id.into(),
            parents: parents.iter().map(|s| s.to_string()).collect(),
            gaming,
            xinput,
            other_input: keyboard,
            known: true,
        }
    }
    #[test]
    fn hid_roles_are_read_from_hardware_ids_even_when_compatible_ids_are_empty() {
        for (property, expected) in [
            ("HID_DEVICE_UP:FFA0_U:0001", (false, false, true)),
            ("HID_DEVICE_SYSTEM_GAME", (true, false, true)),
            ("HID_DEVICE_UP:0001_U:0005", (true, false, true)),
            ("HID_DEVICE_UP:0001_U:0006", (false, true, true)),
            ("HID_DEVICE_SYSTEM_MOUSE", (false, true, true)),
        ] {
            assert_eq!(hid_roles("HID\\PAD", &[property.into()], &[]), expected);
            assert_eq!(hid_roles("HID\\PAD", &[], &[property.into()]), expected);
        }
        assert_eq!(hid_roles("HID\\UNKNOWN", &[], &[]), (false, false, false));
    }
    #[test]
    fn composite_xusb_function_is_hidden_without_its_keyboard_mouse_siblings() {
        let base = r"USB\VID_37D7&PID_2502\FLYDIGI_APEX_6";
        let xusb = r"USB\VID_37D7&PID_2502&MI_00\XUSB";
        let root = r"ROOT\ACPI_HAL\0000";
        let mut all = vec![
            node(
                r"HID\VID_37D7&PID_2502&MI_02\GYRO",
                &[base, root],
                false,
                false,
                false,
            ),
            node(
                r"HID\VID_37D7&PID_2502&IG_00\GAME",
                &[xusb, base, root],
                true,
                false,
                false,
            ),
            node(xusb, &[base, root], true, true, false),
            node(
                r"HID\VID_37D7&PID_2502&MI_01&COL01\KEYBOARD",
                &[base, root],
                false,
                false,
                true,
            ),
            node(
                r"HID\VID_37D7&PID_2502&MI_01&COL02\MOUSE",
                &[base, root],
                false,
                false,
                true,
            ),
            node(
                r"HID\VID_37D7&PID_2502&MI_03\UNKNOWN",
                &[base, root],
                false,
                false,
                false,
            ),
        ];
        all[5].known = false;
        let targets = plan(&all[0].id, &all).unwrap().unwrap();
        assert_eq!(
            targets,
            [all[0].id.clone(), all[1].id.clone(), all[2].id.clone()]
        );
        assert!(!contains(&targets, base));
        for sibling in &all[3..] {
            assert!(!contains(&targets, &sibling.id));
        }
    }
    #[test]
    fn keyboard_or_unknown_children_of_the_xusb_function_are_still_rejected() {
        let base = r"USB\VID_1234&PID_5678\RECEIVER";
        let xusb = r"USB\VID_1234&PID_5678&MI_00\XUSB";
        let mut all = vec![
            node(r"HID\VID_1234&PID_5678\PAD", &[base], true, false, false),
            node(xusb, &[base], true, true, false),
            node(
                r"HID\VID_1234&PID_5678\SHARED",
                &[xusb, base],
                false,
                false,
                true,
            ),
        ];
        assert!(plan(&all[0].id, &all).is_err());
        all[2].other_input = false;
        all[2].known = false;
        assert!(plan(&all[0].id, &all).is_err());
    }
    #[test]
    fn physical_usb_ancestry_can_reach_the_acpi_computer_root() {
        let base = r"USB\VID_37D7&PID_2502\FLYDIGI_APEX_6";
        let hardware = [
            r"USB\ROOT_HUB30\HOST",
            r"PCI\VEN_8086&DEV_7A60\CONTROLLER",
            r"ACPI\PNP0A08\0",
            r"ACPI_HAL\PNP0C08\0",
            r"ROOT\ACPI_HAL\0000",
            r"HTREE\ROOT\0",
        ];
        let mut parents = vec![base];
        parents.extend(hardware);
        let mut virtual_parents = vec![r"ROOT\VIGEMBUS\0000"];
        virtual_parents.extend(hardware);
        let all = vec![
            node(
                r"HID\VID_37D7&PID_2502&MI_02\GYRO",
                &parents,
                false,
                false,
                false,
            ),
            node(
                r"HID\VID_37D7&PID_2502&IG_00\GAME",
                &parents,
                true,
                false,
                false,
            ),
            node(base, &hardware, true, true, false),
            node(
                r"HID\VID_37D7&PID_2502\VIRTUAL",
                &virtual_parents,
                true,
                false,
                false,
            ),
        ];
        let targets = plan(&all[0].id, &all).unwrap().unwrap();
        assert_eq!(targets.len(), 3);
        assert!(contains(&targets, base));
        assert_eq!(virtual_ids(&all), [all[3].id.clone()]);
        assert!(plan(&all[3].id, &all).is_err());
        // A physical target hidden by this lease must not be mistaken for a
        // hidden virtual output just because it has the same computer root.
        assert!(verify(&all, &targets, &[], &[], true, true).is_ok());
    }
    #[test]
    fn hides_physical_hid_and_xusb_but_never_a_same_vid_virtual_pad() {
        let base = r"USB\VID_37D7&PID_2502\ONE";
        let all = vec![
            node(r"HID\VID_37D7&PID_2502\GYRO", &[base], false, false, false),
            node(
                r"HID\VID_37D7&PID_2502&IG_00\GAME",
                &[base],
                true,
                false,
                false,
            ),
            node(base, &[base], true, true, false),
            node(
                r"HID\VID_37D7&PID_2502\VIRTUAL",
                &[r"ROOT\VIGEMBUS\001"],
                true,
                false,
                false,
            ),
        ];
        let plan = plan(&all[0].id, &all).unwrap().unwrap();
        assert_eq!(plan.len(), 3);
        assert!(!contains(&plan, &all[3].id));
        assert!(self::plan(&all[3].id, &all).is_err());
    }
    #[test]
    fn shared_receiver_keyboard_and_ambiguous_xinput_are_rejected() {
        let base = r"USB\VID_1234&PID_5678\ONE";
        let mut all = vec![
            node(r"HID\VID_1234&PID_5678\PAD", &[base], true, false, false),
            node(base, &[base], true, true, false),
            node(
                r"HID\VID_1234&PID_5678\KEYBOARD",
                &[base],
                false,
                false,
                true,
            ),
        ];
        assert!(plan(&all[0].id, &all).is_err());
        all.pop();
        all.push(node(
            r"USB\VID_1234&PID_5678&MI_02\TWO",
            &[base],
            true,
            true,
            false,
        ));
        assert!(plan(&all[0].id, &all).is_err());
    }
    #[test]
    fn existing_hidden_virtual_with_same_vid_is_not_unhidden_or_ignored() {
        let root = r"ROOT\HIDCLASS\MS01";
        let nodes = [node(
            r"HID\VID_054C&PID_09CC\VIRTUAL",
            &[root],
            true,
            false,
            false,
        )];
        assert!(verify(&nodes, &[root.into()], &[root.into()], &[], true, true).is_err());
        assert!(verify(&nodes, &[], &[root.into()], &[], true, true).is_ok());
        assert!(verify(&[], &[], &[root.into()], &[], true, false).is_ok());
    }
}
