#![no_std]
#![no_main]

use cortex_m_rt::entry;
use stm32g4::stm32g474;
use panic_probe as _;
use defmt_rtt as _;

#[entry]
fn main() -> ! {
    defmt::println!("Iniciando prueba DAC...");

    let dp = stm32g474::Peripherals::take().unwrap();

    // === Habilitar relojes ===
    let rcc = dp.RCC;
    rcc.ahb2enr().modify(|_, w| w.gpioaen().set_bit()); // GPIOA
    rcc.apb1enr1().modify(|_, w| w.i2c1en().set_bit());  // DAC1

    // === Configurar PA4 como analógico ===
    let gpioa = dp.GPIOA;
    gpioa.moder().modify(|_, w| w.moder4().analog());

    let dac = dp.DAC1;

    // === Habilitar DAC1 Channel 1 sin trigger ===
    unsafe {
        dac.cr().modify(|_, w| {
            w.en1().set_bit();   // habilitar DAC1 CH1
            w.ten1().clear_bit(); // sin trigger
            w
        });
    }

    // === Escribir valor fijo en DAC ===
    unsafe {
        dac.dhr12r1().write(|w| w.bits(2048)); // mitad de rango
    }

    // === Leer valor DAC ===
    let dac_val = dac.dhr12r1().read().bits();
    defmt::println!("DAC DHR12R1={}", dac_val);

    loop {
        cortex_m::asm::delay(1_000_000);
    }
}

