use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;
use defmt::info;
use defmt::warn;
use embassy_futures::join::join;
use embassy_rp::Peri;
use embassy_rp::bind_interrupts;
use embassy_rp::peripherals::USB;
use embassy_rp::usb::Driver;
use embassy_rp::usb::InterruptHandler;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_usb::Builder;
use embassy_usb::Config;
use embassy_usb::Handler;
use embassy_usb::class::hid::HidReaderWriter;
use embassy_usb::class::hid::ReportId;
use embassy_usb::class::hid::RequestHandler;
use embassy_usb::class::hid::State;
use embassy_usb::control::OutResponse;
use usbd_hid::descriptor::AsInputReport;
use usbd_hid::descriptor::SerializedDescriptor;
use usbd_hid::descriptor::gen_hid_descriptor;
use usbd_hid::descriptor::generator_prelude::Serialize;
use usbd_hid::descriptor::generator_prelude::SerializeTuple;
use usbd_hid::descriptor::generator_prelude::Serializer;

#[gen_hid_descriptor(
    (collection = APPLICATION, usage_page = GENERIC_DESKTOP, usage = JOYSTICK) = {
        (collection = PHYSICAL, usage = GAMEPAD) = {
            (usage_page = BUTTON, usage_min = 1, usage_max = 8) = {
                #[packed_bits 8] #[item_settings data,variable,absolute] buttons=input;
            };
            (usage_page = BUTTON, usage_min = 9, usage_max = 12) = {
                #[packed_bits 4] #[item_settings data,variable,absolute] buttons_menu=input;
            };
            (usage_page = GENERIC_DESKTOP,) = {
                (usage = X, logical_min = 0) = {
                    #[item_settings data,variable,absolute] tt=input;
                };
            };
        };
    }
)]
pub struct KonamiIIDXReport {
    pub buttons: u8,
    pub buttons_menu: u8,
    pub tt: u8,
}

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
});

#[embassy_executor::task]
pub async fn usb_task(
    usb: Peri<'static, USB>,
    buttons: &'static Signal<CriticalSectionRawMutex, u16>,
    encoder: &'static Signal<CriticalSectionRawMutex, u8>,
) {
    let driver = Driver::new(usb, Irqs);

    let mut config = Config::new(0x1CCF, 0x8048);
    config.manufacturer = Some("Konami Amusement");
    config.product = Some("beatmania IIDX controller premium model");
    config.serial_number = Some("12345678");

    let mut config_descriptor = [0; 256];
    let mut bos_descriptor = [0; 256];
    let mut msos_descriptor = [0; 256];
    let mut control_buf = [0; 256];
    let mut request_handler = MyRequestHandler {};
    let mut device_handler = MyDeviceHandler::new();

    let mut state = State::new();

    let mut builder = Builder::new(
        driver,
        config,
        &mut config_descriptor,
        &mut bos_descriptor,
        &mut msos_descriptor,
        &mut control_buf,
    );

    builder.handler(&mut device_handler);

    let config = embassy_usb::class::hid::Config {
        report_descriptor: KonamiIIDXReport::desc(),
        request_handler: None,
        poll_ms: 1,
        max_packet_size: 64,
    };

    let hid = HidReaderWriter::<_, 1, 8>::new(&mut builder, &mut state, config);

    // Build the builder.
    let mut usb = builder.build();

    // Run the USB device.
    let usb_fut = usb.run();

    let (reader, mut writer) = hid.split();

    let in_fut = async {
        let mut encoder_reading = 0;

        loop {
            let buttons_report = buttons.wait().await;

            encoder_reading = match encoder.try_take() {
                None => encoder_reading,
                Some(x) => x,
            };

            let report = KonamiIIDXReport {
                tt: encoder_reading,
                buttons: (buttons_report & 0xFF) as u8,
                buttons_menu: ((buttons_report & 0xFF00) >> 8) as u8,
            };

            // Send the report.
            match writer.write_serialize(&report).await {
                Ok(()) => {}
                Err(e) => warn!("Failed to send report: {:?}", e),
            };
        }
    };

    let out_fut = async {
        reader.run(false, &mut request_handler).await;
    };

    join(usb_fut, join(in_fut, out_fut)).await;
}

struct MyRequestHandler {}

impl RequestHandler for MyRequestHandler {
    fn get_report(&mut self, id: ReportId, _buf: &mut [u8]) -> Option<usize> {
        info!("Get report for {:?}", id);
        None
    }

    fn set_report(&mut self, id: ReportId, data: &[u8]) -> OutResponse {
        info!("Set report for {:?}: {=[u8]}", id, data);
        OutResponse::Accepted
    }

    fn set_idle_ms(&mut self, id: Option<ReportId>, dur: u32) {
        info!("Set idle rate for {:?} to {:?}", id, dur);
    }

    fn get_idle_ms(&mut self, id: Option<ReportId>) -> Option<u32> {
        info!("Get idle rate for {:?}", id);
        None
    }
}

struct MyDeviceHandler {
    configured: AtomicBool,
}

impl MyDeviceHandler {
    fn new() -> Self {
        MyDeviceHandler {
            configured: AtomicBool::new(false),
        }
    }
}

impl Handler for MyDeviceHandler {
    fn enabled(&mut self, enabled: bool) {
        self.configured.store(false, Ordering::Relaxed);
        if enabled {
            info!("Device enabled");
        } else {
            info!("Device disabled");
        }
    }

    fn reset(&mut self) {
        self.configured.store(false, Ordering::Relaxed);
        info!("Bus reset, the Vbus current limit is 100mA");
    }

    fn addressed(&mut self, addr: u8) {
        self.configured.store(false, Ordering::Relaxed);
        info!("USB address set to: {}", addr);
    }

    fn configured(&mut self, configured: bool) {
        self.configured.store(configured, Ordering::Relaxed);
        if configured {
            info!(
                "Device configured, it may now draw up to the configured current limit from Vbus."
            )
        } else {
            info!("Device is no longer configured, the Vbus current limit is 100mA.");
        }
    }
}
