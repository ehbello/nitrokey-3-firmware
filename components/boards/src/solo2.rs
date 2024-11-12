use lpc55_hal::{
    peripherals::ctimer,
    typestates::init_state::Unknown,
};

use memory_regions::MemoryRegions;

use crate::{soc::lpc55::Lpc55, store::impl_storage_pointers, Board};

pub mod button;
pub mod led;
pub mod nfc;
pub mod prince;
pub mod spi;

#[cfg(feature = "no-encrypted-storage")]
use trussed::types::LfsResult;

#[cfg(feature = "no-encrypted-storage")]
lpc55_hal::littlefs2_filesystem!(InternalFilesystem: (prince::FS_START, prince::BLOCK_COUNT));
#[cfg(not(feature = "no-encrypted-storage"))]
use prince::InternalFilesystem;

use nfc::NfcChip;

pub const MEMORY_REGIONS: &MemoryRegions = &MemoryRegions::SOLO2;

pub type PwmTimer = ctimer::Ctimer3<Unknown>;
pub type ButtonsTimer = ctimer::Ctimer1<Unknown>;

pub struct SOLO2;

impl Board for SOLO2 {
    type Soc = Lpc55;

    type NfcDevice = NfcChip;
    type Buttons = button::ThreeButtons;
    type Led = led::RgbLed;

    type Twi = ();
    type Se050Timer = ();

    const BOARD_NAME: &'static str = "solo2";
    const HAS_NFC: bool = true;

    fn prepare_ifs(ifs: &mut Self::InternalStorage) {
        ifs.format_journal_blocks();
    }

    fn recover_ifs(
        ifs_storage: &mut Self::InternalStorage,
        _ifs_alloc: &mut Allocation<Self::InternalStorage>,
        _efs_storage: &mut Self::ExternalStorage,
    ) -> LfsResult<()> {
        error_now!("IFS (solo2) mount-fail");
        ifs_storage.recover_from_journal();
        Ok(())
    }
}

pub type InternalFlashStorage = InternalFilesystem;
impl_storage_pointers!(
    SOLO2,
    Internal = InternalFlashStorage,
);
