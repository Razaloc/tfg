#![no_std]
#![no_main]

use cortex_m_rt::entry;
use stm32g4::stm32g474;
use panic_probe as _;
use defmt_rtt as _;

#[entry]
fn main() -> ! {
    defmt::println!("Prueba DAC → ADC");

    let dp = stm32g474::Peripherals::take().unwrap();

    // Habilitar relojes GPIOA, DAC1, ADC12
    let rcc = dp.RCC;
    rcc.ahb2enr().modify(|_, w| w.gpioaen().set_bit());
    rcc.ahb2enr().modify(|_, w| w.adc12en().set_bit());
    rcc.apb1enr1().modify(|_, w| w.i2c1en().set_bit());

    let gpioa = dp.GPIOA;
    // PA4 (DAC1_OUT1) y PA0 (ADC1_IN1) en modo analog
    gpioa.moder().modify(|_, w| {
        w.moder4().analog();
        w.moder0().analog();
        w
    });

    let dac = dp.DAC1;
    let adc = dp.ADC1;
    let adc_common = dp.ADC12_COMMON;

    // Configurar reloj ADC
    unsafe { adc_common.ccr().modify(|_, w| w.ckmode().bits(0b01)); }

    // Secuencia ADC: deshabilitar, regulator, calibración
    if adc.cr().read().aden().bit_is_set() {
        adc.cr().modify(|_, w| w.addis().set_bit());
        while adc.cr().read().aden().bit_is_set() {}
    }

    adc.cr().modify(|_, w| w.advregen().set_bit());
    cortex_m::asm::delay(30_000);

    adc.cr().modify(|_, w| w.adcal().set_bit());
    while adc.cr().read().adcal().bit_is_set() {}
    cortex_m::asm::delay(20_000);

    // Habilitar ADC
    adc.cr().modify(|_, w| w.aden().set_bit());
    while adc.isr().read().adrdy().bit_is_clear() {}

    // Configurar canal 1 (PA0) y sample time medio
    adc.sqr1().modify(|_, w| unsafe { w.sq1().bits(1) });
    adc.smpr1().modify(|_, w| unsafe { w.smp1().bits(0b010) });

    // Habilitar DAC CH1 sin trigger
    dac.cr().modify(|_, w| {
        w.en1().set_bit();   // enable DAC
        w.ten1().clear_bit(); // trigger deshabilitado, modo software
        w
    });

    // Bucle de prueba: escribir valores DAC y leer ADC
    let test_values = [0u16, 1024, 2048, 3072, 4095];

    loop {
        for &val in test_values.iter() {
            // Escribir valor al DAC
            dac.dhr12r1().write(|w| unsafe {w.bits(val.into())});

            cortex_m::asm::delay(1_000_000); // esperar señal estable

            // Iniciar conversión ADC por software
            adc.cr().modify(|_, w| w.adstart().set_bit());
            while adc.isr().read().eoc().bit_is_clear() {}

            let adc_val = adc.dr().read().bits();
            defmt::println!("DAC={}, ADC={}", val, adc_val);
        }
    }
}

