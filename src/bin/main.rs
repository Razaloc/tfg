#![no_std]
#![no_main]

use cortex_m_rt::entry;
use stm32g4::stm32g474;
use panic_probe as _;
use defmt::info;
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
    info!("Iniciando STM32G474...");
    let dp = stm32g474::Peripherals::take().unwrap();

    // === 1️⃣ Habilitar relojes ===
    let rcc = dp.RCC;
    rcc.ahb2enr().modify(|_, w| w.gpioaen().set_bit());
    rcc.ahb1enr().modify(|_, w| w.dma1en().set_bit());
    rcc.apb1enr1().modify(|_, w| {
        w.tim6en().set_bit()
         .i2c1en().set_bit()
         .i2c1en().set_bit()
    });

    // === 2️⃣ Configurar GPIOA PA4 (DAC) y PA0 (ADC) ===
    let gpioa = dp.GPIOA;
    gpioa.moder().modify(|_, w| {
        w.moder4().analog();
        w.moder0().analog();
        w
    });

    // === 3️⃣ Configurar Timer 6 como trigger (10 kHz aprox) ===
    let tim6 = dp.TIM6;
    unsafe {
        tim6.psc().write(|w| w.psc().bits(169)); // prescaler (170 MHz / 170 = 1 MHz)
        tim6.arr().write(|w| w.arr().bits(100)); // 1 MHz / 100 = 10 kHz
        tim6.cr2().modify(|_, w| w.mms().update()); // TRGO = update event
        tim6.cr1().modify(|_, w| w.cen().set_bit()); // enable
    }

    // === 4️⃣ Configurar DAC1 Channel 1 con trigger de TIM6 ===
    let dac = dp.DAC1;
    unsafe {
        dac.cr().modify(|_, w| {
            w.ten1().set_bit();       // enable trigger
            w.tsel1().bits(0b000);    // TIM6 TRGO
            w.en1().set_bit();
            w
        });
    }

    // === 5️⃣ Configurar DMA1 Channel 3 → DAC1_DHR12R1 ===
    let dma1 = dp.DMA1;
    let dac_dhr12r1_addr = &dac.dhr12r1() as *const _ as u32;

    unsafe {
        let ch = 2; // canal 3 (índice 2)
        dma1.ch(ch).cr().modify(|_, w| w.en().clear_bit());
        dma1.ch(ch).par().write(|w| w.pa().bits(dac_dhr12r1_addr));
        dma1.ch(ch).mar().write(|w| w.ma().bits(SINE_TABLE.as_ptr() as u32));
        dma1.ch(ch).ndtr().write(|w| w.ndt().bits(N_SAMPLES as u16));
        dma1.ch(ch).cr().modify(|_, w| {
            w.minc().set_bit();
            w.circ().set_bit();
            w.dir().set_bit(); // mem→periph
            w.en().set_bit();
            w
        });
    }

    // === 6️⃣ Configurar ADC1 con trigger de TIM6 TRGO y DMA ===
    let adc = dp.ADC1;
    let adc_common = dp.ADC12_COMMON;

    unsafe {
        adc_common.ccr().modify(|_, w| w.ckmode().bits(0b01)); // HCLK/2
    }
    adc.cr().modify(|_, w| w.aden().set_bit());
    while adc.isr().read().adrdy().bit_is_clear() {}

    adc.sqr1().modify(|_, w| unsafe { w.sq1().bits(1) }); // canal 1 (PA0)
    adc.smpr1().modify(|_, w| unsafe { w.smp1().bits(0b010) }); // sample time

    unsafe {
        adc.cfgr().modify(|_, w| {
            w.exten().rising_edge();
            w.extsel().bits(0b0011); // TIM6_TRGO
            w.dmaen().set_bit();
            w.dmacfg().set_bit(); // circular
            w
        });
    }

    // === 7️⃣ Crear buffer ADC seguro con singleton! ===
    let adc_buffer = singleton!(: [u16; N_SAMPLES] = [0; N_SAMPLES]).unwrap();
    let adc_dr_addr = &adc.dr() as *const _ as u32;

    unsafe {
        let ch = 0; // canal 1
        dma1.ch(ch).cr().modify(|_, w| w.en().clear_bit());
        dma1.ch(ch).par().write(|w| w.pa().bits(adc_dr_addr));
        dma1.ch(ch).mar().write(|w| w.ma().bits(adc_buffer.as_ptr() as u32));
        dma1.ch(ch).ndtr().write(|w| w.ndt().bits(N_SAMPLES as u16));
        dma1.ch(ch).cr().modify(|_, w| {
            w.minc().set_bit();
            w.circ().set_bit();
            w.dir().clear_bit(); // periph→mem
            w.en().set_bit();
            w
        });
    }

    // iniciar conversión
    adc.cr().modify(|_, w| w.adstart().set_bit());

    // === 8️⃣ Bucle principal: comparar entrada y salida ===
    loop {
        cortex_m::asm::delay(1_000_000);

        // Crear slice seguro de lectura del buffer DMA
        let adc_slice: &[u16] = unsafe { core::slice::from_raw_parts(adc_buffer.as_ptr(), N_SAMPLES) };
        let avg_adc: u16 = adc_slice.iter().copied().sum::<u16>() / N_SAMPLES as u16;
        let avg_dac: u16 = SINE_TABLE.iter().copied().sum::<u16>() / N_SAMPLES as u16;

        if avg_adc > avg_dac + 50 || avg_adc < avg_dac - 50 {
            cortex_m::asm::bkpt();
        }
    }
}

