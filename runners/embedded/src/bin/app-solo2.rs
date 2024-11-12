#![no_std]
#![no_main]

delog::generate_macros!();

#[rtic::app(device = lpc55_hal::raw, peripherals = true, dispatchers = [PLU, PIN_INT5, PIN_INT7])]
mod app {
    use apdu_dispatch::dispatch::ApduDispatch;
    use boards::{
        init::UsbClasses,
        solo2::{nfc::NfcChip, SOLO2},
        runtime,
        soc::lpc55::{self, monotonic::SystickMonotonic},
        Apps, Trussed,
    };
    use ctaphid_dispatch::dispatch::Dispatch as CtaphidDispatch;
    use embedded_runner_lib::solo2;
    use lpc55_hal::{
        drivers::timer::Elapsed,
        raw::Interrupt,
        time::{DurationExtensions, Microseconds, Milliseconds},
        traits::wg::timer::{Cancel, CountDown},
    };
    use nfc_device::Iso14443;
    use systick_monotonic::Systick;

    type Board = SOLO2;
    type Soc = <Board as boards::Board>::Soc;

    const REFRESH_MILLISECS: Milliseconds = Milliseconds(50);

    #[monotonic(binds = SysTick, default = true)]
    type Monotonic = Systick<1000>;

    #[shared]
    struct Shared {
        usb: Option<UsbClasses>,
        nfc: Option<Iso14443<NfcChip>>,
        apps: Apps<Board>,
    }

    #[local]
    struct Local {}

    #[init]
    fn init(cx: init::Context) -> (Shared, Local, init::Monotonics) {
        let init_result = solo2::init(cx.device, cx.core);
        
        poll::spawn_after(REFRESH_MILLISECS.convert()).ok();

        (
            Shared {
                usb: init_result.usb_classes,
                nfc: init_result.nfc,
                apps: init_result.apps,
            },
            Local {},
            init::Monotonics(cx.core.SYST),
        )
    }

    #[task(priority = 1, shared = [apps, usb, nfc])]
    fn refresh(cx: refresh::Context) {
        let refresh::SharedResources {
            mut apps,
            mut usb,
            mut nfc,
        } = cx.shared;

        apps.lock(|apps| {
            if let Some(usb) = usb.as_mut() {
                runtime::poll_usb(usb, apps);
            }
            if let Some(nfc) = nfc.as_mut() {
                runtime::poll_nfc(nfc, apps);
            }
        });

        refresh::spawn_after(REFRESH_MILLISECS).ok();
    }

    #[task(binds = USB1_NEEDCLK, shared = [usb])]
    fn usb1_needclk(cx: usb1_needclk::Context) {
        let usb1_needclk::SharedResources { mut usb } = cx.shared;
        if let Some(usb) = usb.as_mut() {
            usb.poll();
        }
    }

    #[task(binds = USB1, shared = [usb])]
    fn usb1(cx: usb1::Context) {
        let usb1::SharedResources { mut usb } = cx.shared;
        if let Some(usb) = usb.as_mut() {
            usb.poll();
        }
    }

    #[task(binds = PIN_INT0, shared = [nfc])]
    fn pin_int0(cx: pin_int0::Context) {
        let pin_int0::SharedResources { mut nfc } = cx.shared;
        if let Some(nfc) = nfc.as_mut() {
            nfc.poll();
        }
    }

    #[task(shared = [usb, nfc, apps])]
    fn poll(cx: poll::Context) {
        let poll::SharedResources { mut usb, mut nfc, mut apps } = cx.shared;
        runtime::poll(&mut apps, &mut usb, &mut nfc);
        poll::spawn_after(REFRESH_MILLISECS.convert()).ok();
    }
}
