#![no_std]
#![no_main]

use cortex_m_rt::entry;
use stm32g4::stm32g474;
use panic_probe as _;
use defmt_rtt as _;

#[entry]
fn main() -> ! {
    defmt::println!("Iniciando prueba ADC mínima...");

    let dp = stm32g474::Peripherals::take().unwrap();

    // 1️⃣ Habilitar relojes
    let rcc = dp.RCC;
    rcc.ahb2enr().modify(|_, w| w.gpioaen().set_bit());
    rcc.ahb2enr().modify(|_, w| w.adc12en().set_bit());

    // 2️⃣ Configurar PA0 como analógico
    let gpioa = dp.GPIOA;
    gpioa.moder().modify(|_, w| w.moder0().analog());

    // 3️⃣ Configurar ADC1
    let adc = dp.ADC1;
    let adc_common = dp.ADC12_COMMON;

    // Seleccionar reloj HCLK/2
    unsafe { adc_common.ccr().modify(|_, w| w.ckmode().bits(0b01)); }

    // Asegurarse ADC deshabilitado
    if adc.cr().read().aden().bit_is_set() {
        adc.cr().modify(|_, w| w.addis().set_bit());
        while adc.cr().read().aden().bit_is_set() {}
    }

    // Habilitar regulador y calibrar
    adc.cr().modify(|_, w| w.advregen().set_bit());
    cortex_m::asm::delay(30_000);
    adc.cr().modify(|_, w| w.adcal().set_bit());
    while adc.cr().read().adcal().bit_is_set() {}

    // Habilitar ADC
    adc.cr().modify(|_, w| w.aden().set_bit());
    while adc.isr().read().adrdy().bit_is_clear() {}

    defmt::println!("ADC habilitado y listo para leer PA0.");

    // Configurar canal y tiempo de muestreo
    adc.sqr1().modify(|_, w| unsafe { w.sq1().bits(1) }); // canal 1 = PA0
    adc.smpr1().modify(|_, w| unsafe { w.smp1().bits(0b010) }); // sample time medio

    // 4️⃣ Bucle de lectura manual
    loop {
        adc.cr().modify(|_, w| w.adstart().set_bit()); // iniciar conversión
        while adc.isr().read().eoc().bit_is_clear() {} // esperar fin conversión
        let val = adc.dr().read().bits();              // leer valor
        defmt::println!("ADC manual PA0 = {}", val);
        cortex_m::asm::delay(1_000_000);
    }
}

