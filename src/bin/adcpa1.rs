#![no_std]
#![no_main]

use cortex_m_rt::entry;
use defmt_rtt as _;
use panic_probe as _;
use stm32g4::stm32g474;

#[entry]
fn main() -> ! {
    defmt::println!("Test: blink en PA1 (A1 Arduino).");

    let dp = stm32g474::Peripherals::take().unwrap();

    // 1) Habilitar GPIOA
    let rcc = dp.RCC;
    rcc.ahb2enr().modify(|_, w| w.gpioaen().set_bit());

    // 2) Configurar PA1 como salida
    let gpioa = dp.GPIOA;
    gpioa.moder().modify(|_, w| w.moder1().output()); // PA1 -> salida push-pull
    gpioa.otyper().modify(|_, w| w.ot1().clear_bit()); // push-pull
    gpioa.ospeedr().modify(|_, w| w.ospeedr1().low_speed());
    gpioa.pupdr().modify(|_, w| w.pupdr1().floating());

    defmt::println!("PA1 configurado como salida. Deberías ver parpadeo en A1.");

    loop {
        // Encender (poner PA1 a 3.3V)
        gpioa.odr().modify(|_, w| w.odr1().set_bit());
        cortex_m::asm::delay(8_000_000); // ~500 ms

        // Apagar (poner PA1 a 0V)
        gpioa.odr().modify(|_, w| w.odr1().clear_bit());
        cortex_m::asm::delay(8_000_000); // ~500 ms
    }
}

