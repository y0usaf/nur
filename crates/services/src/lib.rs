pub mod applications;
pub mod audio;
pub mod battery;
pub mod bluetooth;
pub mod compositor;
pub mod mpris;
pub mod network;
pub mod notifications;
pub mod power_profiles;
pub mod sys_info;
pub mod system_tray;

pub use applications::{
    AppEntry, ApplicationsService, ApplicationsState, search_apps, strip_field_codes,
};
pub use audio::{AudioService, AudioState};
pub use battery::{BatteryService, BatteryState};
pub use bluetooth::{BluetoothDevice, BluetoothService, BluetoothState};
pub use compositor::{CompositorService, CompositorState};
pub use mpris::{MprisService, MprisState};
pub use network::{NetworkService, NetworkState};
pub use notifications::{Notification, NotificationsService, NotificationsState};
pub use power_profiles::{PowerProfilesService, PowerProfilesState};
pub use sys_info::{SysInfoService, SysInfoState};
pub use system_tray::{SystemTrayService, SystemTrayState, TrayItem};
