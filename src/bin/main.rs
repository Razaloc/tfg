#![no_std]
#![no_main]

use cortex_m_rt::entry;
use stm32g4::stm32g474;
use panic_probe as _;
use defmt_rtt as _;
use cortex_m::singleton;

const N_SAMPLES: usize = 32;
const SINE_TABLE: [u16; N_SAMPLES] = [
    2048, 2447, 2831, 3185, 3495, 3750, 3939, 4056,
    4095, 4056, 3939, 3750, 3495, 3185, 2831, 2447,
    2048, 1649, 1265, 911, 601, 346, 157, 40,
    0, 40, 157, 346, 601, 911, 1265, 1649,
];

#[entry]
fn main() -> ! {
    defmt::println!("Iniciando STM32G474...");
    let dp = stm32g474::Peripherals::take().unwrap();

    // === 1️⃣ Relojes ===
    let rcc = dp.RCC;
    rcc.ahb2enr().modify(|_, w| {
        w.gpioaen().set_bit();
        w.adc12en().set_bit();
        w
    });
    rcc.ahb1enr().modify(|_, w| w.dma1en().set_bit());
    rcc.apb1enr1().modify(|_, w| w.tim6en().set_bit());

    // === 2️⃣ GPIO (PA4 DAC, PA0 ADC) ===
    let gpioa = dp.GPIOA;
    gpioa.moder().modify(|_, w| {
        w.moder4().analog();
        w.moder0().analog();
        w
    });

    // === 3️⃣ TIM6 para DAC a 10 kHz ===
    let tim6 = dp.TIM6;
    unsafe {
        tim6.psc().write(|w| w.psc().bits(169));
        tim6.arr().write(|w| w.arr().bits(100));
        tim6.cr2().modify(|_, w| w.mms().bits(0b010)); // TRGO = update
        tim6.cr1().modify(|_, w| w.cen().set_bit());
    }

    // === 4️⃣ DAC1 con trigger TIM6 ===
    let dac = dp.DAC1;
    unsafe {
        dac.dhr12r1().write(|w| w.bits(2048));
        dac.cr().modify(|_, w| {
            w.en1().set_bit();
            w.ten1().set_bit();
            w.tsel1().bits(0b001); // TIM6_TRGO
            w
        });
    }

    // === 5️⃣ DMA1 CH3 → DAC (tabla seno) ===
    let dma1 = dp.DMA1;
    let dac_dhr12 = &dac.dhr12r1() as *const _ as u32;

    unsafe {
        let ch = 2; // CH3
        dma1.ch(ch).cr().modify(|_, w| w.en().clear_bit());
        dma1.ch(ch).par().write(|w| w.pa().bits(dac_dhr12));
        dma1.ch(ch).mar().write(|w| w.ma().bits(SINE_TABLE.as_ptr() as u32));
        dma1.ch(ch).ndtr().write(|w| w.ndt().bits(N_SAMPLES as u16));

        dma1.ch(ch).cr().modify(|_, w| {
            w.minc().set_bit();
            w.circ().set_bit();
            w.dir().set_bit();   // mem → periph
            w.msize().bits(0b01);
            w.psize().bits(0b01);
            w.pl().bits(0b10);
            w.en().set_bit();
            w
        });
    }

    // *****************************************************************
    // === 6️⃣ ADC1 inicialización (con BULB + SMPPLUS) ===
    // *****************************************************************

    let adc = dp.ADC1;
    let adc_common = dp.ADC12_COMMON;

    unsafe { adc_common.ccr().modify(|_, w| w.ckmode().bits(0b01)); }

    // Apagar ADC si está activo
    if adc.cr().read().aden().bit_is_set() {
        adc.cr().modify(|_, w| w.addis().set_bit());
        while adc.cr().read().aden().bit_is_set() {}
    }

    // Deep power-down off
    adc.cr().modify(|_, w| w.deeppwd().clear_bit());

    // Regulador ADC on
    adc.cr().modify(|_, w| w.advregen().set_bit());
    cortex_m::asm::delay(30_000);

    // Calibración
    adc.cr().modify(|_, w| w.adcal().set_bit());
    while adc.cr().read().adcal().bit_is_set() {}
    cortex_m::asm::delay(20_000);

    // === Canal y sampling ===
    adc.sqr1().modify(|_, w| unsafe {
        w.l().bits(0);
        w.sq1().bits(1) // ADC1_IN1 = PA0
    });

    unsafe {
        adc.smpr1().modify(|_, w| {
            w.smp1().bits(0b010);
            w.smpplus().set_bit();  // <--- Stability fix #1
            w
        });
    }

    // === BULB sampling mode (errata workaround) ===
    adc.cfgr2().modify(|_, w| w.bulb().set_bit()); // <--- Stability fix #2

    // === CFGR antes de ADEN ===
    adc.cfgr().modify(|_, w| {
        w.cont().set_bit();       // modo continuo
        w.exten().disabled();     // sin trigger
        w.dmaen().set_bit();      // DMA enable
        w.dmacfg().set_bit();     // circular
        w
    });

    // Habilitar ADC
    adc.isr().write(|w| w.adrdy().clear());
    adc.cr().modify(|_, w| w.aden().set_bit());
    while adc.isr().read().adrdy().bit_is_clear() {}

    defmt::println!("¡ADC y DAC configurados!");

    // === 7️⃣ DMA1 CH1 → ADC_DR ===
    let adc_buffer = singleton!(: [u16; N_SAMPLES] = [0; N_SAMPLES]).unwrap();
    let adc_dr = &adc.dr() as *const _ as u32;

    unsafe {
        let ch = 0;
        dma1.ch(ch).cr().modify(|_, w| w.en().clear_bit());
        dma1.ch(ch).par().write(|w| w.pa().bits(adc_dr));
        dma1.ch(ch).mar().write(|w| w.ma().bits(adc_buffer.as_ptr() as u32));
        dma1.ch(ch).ndtr().write(|w| w.ndt().bits(N_SAMPLES as u16));

        dma1.ch(ch).cr().modify(|_, w| {
            w.minc().set_bit();
            w.circ().set_bit();
            w.dir().clear_bit();   // periph → mem
            w.msize().bits(0b01);
            w.psize().bits(0b01);
            w.pl().bits(0b10);
            w.en().set_bit();
            w
        });
    }

    // Start ADC conversions
    adc.cr().modify(|_, w| w.adstart().set_bit());
    defmt::println!("¡ADC en marcha!");

    // === 8️⃣ Loop ===
    loop {
        cortex_m::asm::delay(1_000_000);

        let adc_slice =
            unsafe { core::slice::from_raw_parts(adc_buffer.as_ptr(), N_SAMPLES) };

        let avg_adc: u16 = adc_slice.iter().copied().sum::<u16>() / N_SAMPLES as u16;
        let avg_dac: u16 = SINE_TABLE.iter().copied().sum::<u16>() / N_SAMPLES as u16;

        defmt::println!("ADC primeros valores: {:?}", &adc_slice[..8]);
        defmt::println!("Promedios: ADC={}, DAC={}", avg_adc, avg_dac);

        if (avg_adc as i32 - avg_dac as i32).abs() > 50 {
            defmt::warn!("Desviación alta: ADC={}, DAC={}", avg_adc, avg_dac);
        }
    }
}

