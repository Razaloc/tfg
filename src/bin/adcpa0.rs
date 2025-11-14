#![no_std]
#![no_main]

use cortex_m_rt::entry;
use stm32g4::stm32g474;
use panic_probe as _;
use defmt_rtt as _;

#[entry]
fn main() -> ! {
    defmt::println!("Iniciando prueba mínima ADC...");

    let dp = stm32g474::Peripherals::take().unwrap();

    // === 1️⃣ Relojes ===
    let rcc = dp.RCC;

    // Habilitar GPIOA y ADC12
    rcc.ahb2enr().modify(|_, w| {
        w.gpioaen().set_bit();
        w.adc12en().set_bit()
    });

    // === 2️⃣ PA0 como analógico (ADC1_IN1) ===
    let gpioa = dp.GPIOA;
    gpioa.moder().modify(|_, w| {
        w.moder0().analog(); // PA0 analógico
        w
    });

    // === 3️⃣ Configurar reloj de ADC ===
    let adc_common = dp.ADC12_COMMON;
    unsafe {
        // CKMODE = 01: HCLK/1 (o HCLK/2 según RM; sirve para la prueba)
        adc_common.ccr().modify(|_, w| w.ckmode().bits(0b01));
    }

    // === 4️⃣ Inicialización de ADC1 ===
    let adc = dp.ADC1;

    // Asegurarnos de que está deshabilitado
    if adc.cr().read().aden().bit_is_set() {
        adc.cr().modify(|_, w| w.addis().set_bit());
        while adc.cr().read().aden().bit_is_set() {}
    }

    // 4.1 Salir de deep power-down
    adc.cr().modify(|_, w| w.deeppwd().clear_bit());

    // 4.2 Encender regulador interno
    adc.cr().modify(|_, w| w.advregen().set_bit());
    cortex_m::asm::delay(30_000); // ~20us aprox

    // 4.3 Calibración
    // (modo single-ended por defecto; difsel=0 para todos)
    adc.cr().modify(|_, w| w.adcal().set_bit());
    while adc.cr().read().adcal().bit_is_set() {}
    cortex_m::asm::delay(20_000);

    // 4.4 Habilitar ADC y esperar ADRDY
    adc.isr().write(|w| w.adrdy().clear());
    adc.cr().modify(|_, w| w.aden().set_bit());
    while adc.isr().read().adrdy().bit_is_clear() {}

    defmt::println!("ADC habilitado, ADRDY = {}", adc.isr().read().adrdy().bit());

    // === 5️⃣ Configurar canal y tiempo de muestreo ===
    // Secuencia: 1 conversión, canal 1 (PA0 = ADC1_IN1)
    adc.sqr1().modify(|_, w| unsafe {
        w.l().bits(0);     // 0 => 1 conversión en total
        w.sq1().bits(1);   // canal 1
        w
    });

    // Tiempo de muestreo más largo para asegurar estabilidad (ej: 47.5 ciclos)
    unsafe {
        adc.smpr1().modify(|_, w| w.smp1().bits(0b010)); // o 0b111 para más largo
    }

    // === 6️⃣ Configurar modo continuo, sin trigger externo, sin DMA ===
    adc.cfgr().modify(|_, w| {
        w.cont().set_bit();       // modo continuo
        w.exten().disabled();     // sin trigger externo
        w
    });

    // === 7️⃣ Empezar conversiones ===
    adc.cr().modify(|_, w| w.adstart().set_bit());

    defmt::println!("ADSTART = {}", adc.cr().read().adstart().bit());

    // === 8️⃣ Bucle principal: leer DR directamente ===
    loop {
        // Esperar fin de conversión
        //while adc.isr().read().eoc().bit_is_clear() {}

        let val = adc.dr().read().rdata().bits();
        defmt::println!("ADC_DR = {}", val);

        // Limpiar flag EOC (a veces se limpia al leer DR; si no, forzamos)
        adc.isr().write(|w| w.eoc().clear());

        cortex_m::asm::delay(1_000_000);
    }
}

