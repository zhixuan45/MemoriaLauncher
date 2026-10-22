use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryMode {
    Automatic,
    Custom,
    /// Older settings keep their exact MiB allocation until the user moves the slider.
    #[default]
    Fixed,
}

#[derive(Debug, Clone, Copy)]
pub struct MemorySnapshot {
    pub total_mb: u64,
    pub available_mb: u64,
}

impl MemorySnapshot {
    pub fn read() -> Self {
        let mut system = sysinfo::System::new();
        system.refresh_memory();
        let total_mb = system.total_memory() / (1024 * 1024);
        Self {
            total_mb,
            available_mb: (system.available_memory() / (1024 * 1024)).min(total_mb),
        }
    }

    pub fn used_mb(self) -> u64 {
        self.total_mb.saturating_sub(self.available_mb)
    }

    pub fn allocation(self, mode: MemoryMode, percent: u8, fixed_mb: u32) -> Option<u32> {
        if mode == MemoryMode::Fixed {
            return Some(fixed_mb);
        }
        if self.total_mb == 0 {
            return None;
        }
        let target = if mode == MemoryMode::Custom {
            self.total_mb.saturating_mul(percent.clamp(1, 90) as u64) / 100
        } else {
            // Keep half of currently available RAM and cap automatic heaps at 16 GiB.
            let target = (self.total_mb / 4)
                .min(self.available_mb / 2)
                .min(16 * 1024);
            target / 128 * 128
        };
        Some(target.clamp(128, 1024 * 1024) as u32)
    }
}

pub fn gib(mb: u64) -> String {
    format!("{:.1} GB", mb as f64 / 1024.)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_allocation_tracks_available_ram_and_custom_ratio_tracks_total_ram() {
        let roomy = MemorySnapshot {
            total_mb: 32768,
            available_mb: 24576,
        };
        let busy = MemorySnapshot {
            total_mb: 32768,
            available_mb: 8192,
        };
        assert_eq!(
            roomy.allocation(MemoryMode::Automatic, 25, 4096),
            Some(8192)
        );
        assert_eq!(busy.allocation(MemoryMode::Automatic, 25, 4096), Some(4096));
        for snapshot in [roomy, busy] {
            assert_eq!(
                snapshot.allocation(MemoryMode::Custom, 30, 4096),
                Some(9830)
            );
            assert_eq!(snapshot.allocation(MemoryMode::Fixed, 30, 6144), Some(6144));
        }
        assert_eq!(
            MemorySnapshot {
                total_mb: 262144,
                available_mb: 262144
            }
            .allocation(MemoryMode::Automatic, 25, 4096),
            Some(16384)
        );
        assert_eq!(
            MemorySnapshot {
                total_mb: 0,
                available_mb: 0
            }
            .allocation(MemoryMode::Automatic, 25, 4096),
            None
        );
    }
}
