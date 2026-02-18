use crate::sysfs::SysfsRoot;

#[derive(Debug, Clone, Default)]
pub struct DmiInfo {
    pub board_vendor: Option<String>,
    pub board_name: Option<String>,
    pub product_name: Option<String>,
    pub product_family: Option<String>,
    pub bios_version: Option<String>,
}

impl DmiInfo {
    pub fn detect(sysfs: &SysfsRoot) -> Self {
        Self {
            board_vendor: sysfs.read_optional("sys/class/dmi/id/board_vendor").unwrap_or(None),
            board_name: sysfs.read_optional("sys/class/dmi/id/board_name").unwrap_or(None),
            product_name: sysfs.read_optional("sys/class/dmi/id/product_name").unwrap_or(None),
            product_family: sysfs.read_optional("sys/class/dmi/id/product_family").unwrap_or(None),
            bios_version: sysfs.read_optional("sys/class/dmi/id/bios_version").unwrap_or(None),
        }
    }

    pub fn is_framework(&self) -> bool {
        self.board_vendor
            .as_deref()
            .is_some_and(|v| v.contains("Framework"))
    }

    pub fn is_framework_16(&self) -> bool {
        self.is_framework()
            && (self
                .product_name
                .as_deref()
                .is_some_and(|n| n.contains("16"))
                || self
                    .board_name
                    .as_deref()
                    .is_some_and(|n| n.contains("16")))
    }
}
