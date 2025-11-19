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
    rcc.ahb1enr().modify(|_, w| {
        w.dma1en().set_bit();
        w.dmamux1en().set_bit();
        w
    });
    rcc.ahb2enr().modify(|_, w| {
        w.dac1en().set_bit();    // habilita DAC1/DAC2
        w.gpioaen().set_bit();
        w.adc12en().set_bit();
        w
    });

    rcc.apb1enr1().modify(|_, w| {
        w.tim6en().set_bit();   // habilita TIM6 en APB1
        w
    });

    // === 2️⃣ GPIO ===
    let gpioa = dp.GPIOA;
    gpioa.moder().modify(|_, w| {
        w.moder4().analog(); // DAC1_OUT1 (PA4)
        w.moder0().analog(); // ADC1_IN1  (PA0)
        w
    });

    // === 3️⃣ TIM6: 10 kHz para trigger del DAC ===
    let tim6 = dp.TIM6;
    unsafe {
        tim6.psc().write(|w| w.psc().bits(169));
        tim6.arr().write(|w| w.arr().bits(100));
        tim6.cr2().modify(|_, w| w.mms().bits(0b010)); // TRGO = update
        tim6.dier().modify(|_, w| w.ude().set_bit());
        tim6.cr1().modify(|_, w| w.cen().set_bit());
    }

    // === 4️⃣ DAC1 CH1 con trigger TIM6 ===
    let dac = dp.DAC1;
    unsafe {
        dac.dhr12r1().write(|w| w.bits(2048));
        dac.cr().modify(|_, w| {
            w.en1().set_bit();
            w.ten1().set_bit();
            w.dmaen1().set_bit(); 
            w.tsel1().tim6trgo(); // TIM6_TRGO
            w
        });
    }

    // === 5️⃣ DMA1 CH3 → DAC (tabla seno) ===
    let dma1 = dp.DMA1;
    // Puntero correcto al registro del DAC para DMA
    let dac_dhr12_addr = dac.dhr12r1().as_ptr() as u32;

    unsafe {
        let ch = 2; // CH3
        dma1.ch(ch).cr().modify(|_, w| w.en().clear_bit());
        dma1.ch(ch).par().write(|w| w.pa().bits(dac_dhr12_addr));
        dma1.ch(ch).mar().write(|w| w.ma().bits(SINE_TABLE.as_ptr() as u32));
        dma1.ch(ch).ndtr().write(|w| w.ndt().bits(N_SAMPLES as u16));

        dma1.ch(ch).cr().modify(|_, w| {
            w.minc().set_bit();
            w.circ().set_bit();
            w.dir().set_bit();   // mem → periph
            w.msize().bits(0b01); // 16 bits
            w.psize().bits(0b10);
            w.pl().bits(0b10);
            w.en().set_bit();
            w
        });
    }

    // === 5.5️⃣ DMAMUX: ADC12 → DMA1_CH1 ===
    let dmamux = dp.DMAMUX;
    unsafe {
        // ya tienes:
        dmamux.ccr(0).modify(|_, w| {
            w.dmareq_id().bits(5); // ADC1/2 → DMA1_CH1
            w
        });

        // NUEVO: DAC1_CH1 → DMA1_CH3 (canal 2 del DMA)
        dmamux.ccr(2).modify(|_, w| {
            w.dmareq_id().bits(6); // 6 = DAC1_CH1
            w
        });
    }


    // === 6️⃣ ADC1 inicialización ===
    let adc = dp.ADC1;
    let adc_common = dp.ADC12_COMMON;

    unsafe { adc_common.ccr().modify(|_, w| w.ckmode().bits(0b01)); }

    if adc.cr().read().aden().bit_is_set() {
        adc.cr().modify(|_, w| w.addis().set_bit());
        while adc.cr().read().aden().bit_is_set() {}
    }

    adc.cr().modify(|_, w| w.deeppwd().clear_bit());
    adc.cr().modify(|_, w| w.advregen().set_bit());
    cortex_m::asm::delay(30_000);

    adc.cr().modify(|_, w| w.adcal().set_bit());
    while adc.cr().read().adcal().bit_is_set() {}
    cortex_m::asm::delay(20_000);

    // Canal + muestreo
    adc.sqr1().modify(|_, w| unsafe {
        w.l().bits(0);
        w.sq1().bits(1) // canal 1 = PA0
    });
    unsafe {
        adc.smpr1().modify(|_, w| {
            w.smp1().bits(0b010); // sampling time moderado
            w
        });
    }

    // Modo continuo + DMA
    adc.cfgr().modify(|_, w| {
        w.cont().clear_bit();      // sin modo continuo
        w.dmaen().set_bit();
        w.dmacfg().set_bit();
        w.extsel().tim6_trgo();
        w.exten().rising_edge();   // trigger en flanco de subida
        w
    });


    adc.isr().write(|w| w.adrdy().clear());
    adc.cr().modify(|_, w| w.aden().set_bit());
    //while adc.isr().read().adrdy().bit_is_clear() {}

    //defmt::println!("¡ADC y DAC configurados!");

    // === 7️⃣ DMA1 CH1 → ADC_DR (periph→mem) ===
    let adc_buffer = singleton!(: [u16; N_SAMPLES] = [0; N_SAMPLES]).unwrap();
    // Puntero correcto al registro DR para DMA
    let adc_dr_addr = adc.dr().as_ptr() as u32;

    unsafe {
        let ch = 0; // CH1
        dma1.ch(ch).cr().modify(|_, w| w.en().clear_bit());
        dma1.ch(ch).par().write(|w| w.pa().bits(adc_dr_addr));
        dma1.ch(ch).mar().write(|w| w.ma().bits(adc_buffer.as_ptr() as u32));
        dma1.ch(ch).ndtr().write(|w| w.ndt().bits(N_SAMPLES as u16));

        dma1.ch(ch).cr().modify(|_, w| {
            w.minc().set_bit();
            w.circ().set_bit();
            w.dir().clear_bit();   // periph → mem
            w.msize().bits(0b01);  // 16 bits en memoria
            w.psize().bits(0b10);  
            w.pl().bits(0b10);
            w.en().set_bit();
            w
        });
    }

    // Arrancar conversiones
    adc.cr().modify(|_, w| w.adstart().set_bit());
    defmt::println!("¡ADC en marcha!");

    // === 8️⃣ Bucle principal ===
    loop {
        cortex_m::asm::delay(1_000_000);

        //let direct = adc.dr().read().rdata().bits();
        //defmt::println!("ADC_DR DIRECT = {}", direct);
        let adc_slice =
            unsafe { core::slice::from_raw_parts(adc_buffer.as_ptr(), N_SAMPLES) };

        defmt::println!("ADC buffer completo: {:?}", adc_slice);
        //let adc_slice =
        //    unsafe { core::slice::from_raw_parts(adc_buffer.as_ptr(), N_SAMPLES) };

        // Promedio ADC sin overflow
        let sum_adc: u32 = adc_slice.iter().map(|&x| x as u32).sum();
        let avg_adc: u16 = (sum_adc / N_SAMPLES as u32) as u16;

        // Promedio DAC fijo (tabla conocida)
        let avg_dac: u16 = 2048; // o  (SINE_TABLE.iter().map(|&x| x as u32).sum::<u32>() / N_SAMPLES as u32) as u16


        //defmt::println!("ADC primeros valores: {:?}", &adc_slice[..8]);
        //defmt::println!("Promedios: ADC={}, DAC={}", avg_adc, avg_dac);

        if (avg_adc as i32 - avg_dac as i32).abs() > 50 {
            defmt::warn!("Desviación alta: ADC={}, DAC={}", avg_adc, avg_dac);
        }
    }
}

