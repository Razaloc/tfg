#![no_std]
#![no_main]

use cortex_m::singleton;
use cortex_m_rt::entry;
use defmt_rtt as _;
use panic_probe as _;
use stm32g4::stm32g474::{self, gpioc};

struct PerfilFrecuencia {
    frecuencia_hz: u32,
    psc: u16,
    arr: u32,
}

type GpioC = stm32g4::Periph<gpioc::RegisterBlock, 1207961600>;

const N_MUESTRAS: usize = 32;
const PERFIL_INICIAL: usize = 2;

const TIM6_PSC_INICIAL: u16 = 169;
const TIM6_ARR_INICIAL: u32 = 100;
const TIM6_TRGO_UPDATE: u8 = 0b010;

const DAC_VALOR_MEDIO: u32 = 2048;

const DMA_CH_ADC: usize = 0;
const DMA_CH_DAC: usize = 2;
const DMA_MEMORIA_16_BITS: u8 = 0b01;
const DMA_PERIFERICO_32_BITS: u8 = 0b10;
const DMA_PRIORIDAD_ALTA: u8 = 0b10;
const DMAMUX_REQ_ADC12: u8 = 5;
const DMAMUX_REQ_DAC1_CH1: u8 = 6;

const ADC_CANAL_PA0: u8 = 1;
const ADC_CKMODE_PCLK_DIV2: u8 = 0b01;
const ADC_SAMPLING_MODERADO: u8 = 0b010;
const DELAY_ADC_REGULADOR: u32 = 30_000;
const DELAY_ADC_CALIBRACION: u32 = 20_000;

const DELAY_BUCLE_PRINCIPAL: u32 = 1_000_000;

const TABLA_SENO: [u16; N_MUESTRAS] = [
    2048, 2447, 2831, 3185, 3495, 3750, 3939, 4056, 4095, 4056, 3939, 3750, 3495,
    3185, 2831, 2447, 2048, 1649, 1265, 911, 601, 346, 157, 40, 0, 40, 157, 346,
    601, 911, 1265, 1649,
];

const PERFILES: [PerfilFrecuencia; 7] = [
    PerfilFrecuencia { frecuencia_hz: 5,    psc: 169, arr: 6249, },
    PerfilFrecuencia { frecuencia_hz: 50,   psc: 169, arr: 624, },
    PerfilFrecuencia { frecuencia_hz: 100,  psc: 169, arr: 312, },
    PerfilFrecuencia { frecuencia_hz: 250,  psc: 169, arr: 124, },
    PerfilFrecuencia { frecuencia_hz: 500,  psc: 169, arr: 62, },
    PerfilFrecuencia { frecuencia_hz: 1000, psc: 169, arr: 30, },
    PerfilFrecuencia { frecuencia_hz: 3000, psc: 169, arr: 9, },
];

fn habilitar_relojes(rcc: &stm32g474::RCC) {
    rcc.ahb1enr().modify(|_, w| {
        w.dma1en().set_bit();
        w.dmamux1en().set_bit();
        w
    });

    rcc.ahb2enr().modify(|_, w| {
        w.dac1en().set_bit();
        w.gpioaen().set_bit();
        w.adc12en().set_bit();
        w.gpiocen().set_bit();
        w
    });

    rcc.apb1enr1().modify(|_, w| {
        w.tim6en().set_bit();
        w
    });
}

fn configurar_gpioa(gpioa: &stm32g474::GPIOA) {
    gpioa.moder().modify(|_, w| {
        w.moder4().analog(); // DAC1_OUT1 (PA4)
        w.moder0().analog(); // ADC1_IN1 (PA0)
        w
    });
}

fn configurar_boton(gpioc: &GpioC) {
    gpioc.moder().modify(|_, w| {
        w.moder13().input();
        w
    });
}

fn configurar_tim6(tim6: &stm32g474::TIM6) {
    unsafe {
        tim6.psc().write(|w| w.psc().bits(TIM6_PSC_INICIAL));
        tim6.arr().write(|w| w.arr().bits(TIM6_ARR_INICIAL));
        tim6
            .cr2()
            .modify(|_, w| w.mms().bits(TIM6_TRGO_UPDATE));
        tim6.dier().modify(|_, w| w.ude().set_bit());
        tim6.cr1().modify(|_, w| w.cen().set_bit());
    }
}

fn aplicar_perfil_frecuencia(tim6: &stm32g474::TIM6, perfil: &PerfilFrecuencia) {
    unsafe {
        tim6.cr1().modify(|_, w| w.cen().clear_bit());
        tim6.psc().write(|w| w.psc().bits(perfil.psc));
        tim6.arr().write(|w| w.arr().bits(perfil.arr));
        tim6.cnt().write(|w| w.cnt().bits(0));
        tim6.egr().write(|w| w.ug().set_bit());
        tim6
            .cr2()
            .modify(|_, w| w.mms().bits(TIM6_TRGO_UPDATE));
        tim6.cr1().modify(|_, w| w.cen().set_bit());
    }
}

fn configurar_dac(dac: &stm32g474::DAC1) {
    unsafe {
        dac.dhr12r1().write(|w| w.bits(DAC_VALOR_MEDIO));
        dac.cr().modify(|_, w| {
            w.en1().set_bit();
            w.ten1().set_bit();
            w.dmaen1().set_bit();
            w.tsel1().tim6trgo();
            w
        });
    }
}

fn configurar_dma_dac(dma1: &stm32g474::DMA1, dac: &stm32g474::DAC1) {
    let dac_dhr12_addr = dac.dhr12r1().as_ptr() as u32;

    unsafe {
        dma1.ch(DMA_CH_DAC).cr().modify(|_, w| w.en().clear_bit());
        dma1
            .ch(DMA_CH_DAC)
            .par()
            .write(|w| w.pa().bits(dac_dhr12_addr));
        dma1
            .ch(DMA_CH_DAC)
            .mar()
            .write(|w| w.ma().bits(TABLA_SENO.as_ptr() as u32));
        dma1
            .ch(DMA_CH_DAC)
            .ndtr()
            .write(|w| w.ndt().bits(N_MUESTRAS as u16));

        dma1.ch(DMA_CH_DAC).cr().modify(|_, w| {
            w.minc().set_bit();
            w.circ().set_bit();
            w.dir().set_bit();
            w.msize().bits(DMA_MEMORIA_16_BITS);
            w.psize().bits(DMA_PERIFERICO_32_BITS);
            w.pl().bits(DMA_PRIORIDAD_ALTA);
            w.en().set_bit();
            w
        });
    }
}

fn configurar_dmamux(dmamux: &stm32g474::DMAMUX) {
    unsafe {
        dmamux.ccr(DMA_CH_ADC).modify(|_, w| {
            w.dmareq_id().bits(DMAMUX_REQ_ADC12);
            w
        });

        dmamux.ccr(DMA_CH_DAC).modify(|_, w| {
            w.dmareq_id().bits(DMAMUX_REQ_DAC1_CH1);
            w
        });
    }
}

fn configurar_adc(adc: &stm32g474::ADC1, adc_common: &stm32g474::ADC12_COMMON) {
    unsafe {
        adc_common
            .ccr()
            .modify(|_, w| w.ckmode().bits(ADC_CKMODE_PCLK_DIV2));
    }

    if adc.cr().read().aden().bit_is_set() {
        adc.cr().modify(|_, w| w.addis().set_bit());
        while adc.cr().read().aden().bit_is_set() {}
    }

    adc.cr().modify(|_, w| w.deeppwd().clear_bit());
    adc.cr().modify(|_, w| w.advregen().set_bit());
    cortex_m::asm::delay(DELAY_ADC_REGULADOR);

    adc.cr().modify(|_, w| w.adcal().set_bit());
    while adc.cr().read().adcal().bit_is_set() {}
    cortex_m::asm::delay(DELAY_ADC_CALIBRACION);

    adc.sqr1().modify(|_, w| unsafe {
        w.l().bits(0);
        w.sq1().bits(ADC_CANAL_PA0)
    });

    unsafe {
        adc.smpr1().modify(|_, w| {
            w.smp1().bits(ADC_SAMPLING_MODERADO);
            w
        });
    }

    adc.cfgr().modify(|_, w| {
        w.cont().clear_bit();
        w.dmaen().set_bit();
        w.dmacfg().set_bit();
        w.extsel().tim6_trgo();
        w.exten().rising_edge();
        w
    });

    adc.isr().write(|w| w.adrdy().clear());
    adc.cr().modify(|_, w| w.aden().set_bit());
}

fn configurar_dma_adc(
    dma1: &stm32g474::DMA1,
    adc: &stm32g474::ADC1,
    adc_buffer: &[u16; N_MUESTRAS],
) {
    let adc_dr_addr = adc.dr().as_ptr() as u32;

    unsafe {
        dma1.ch(DMA_CH_ADC).cr().modify(|_, w| w.en().clear_bit());
        dma1
            .ch(DMA_CH_ADC)
            .par()
            .write(|w| w.pa().bits(adc_dr_addr));
        dma1
            .ch(DMA_CH_ADC)
            .mar()
            .write(|w| w.ma().bits(adc_buffer.as_ptr() as u32));
        dma1
            .ch(DMA_CH_ADC)
            .ndtr()
            .write(|w| w.ndt().bits(N_MUESTRAS as u16));

        dma1.ch(DMA_CH_ADC).cr().modify(|_, w| {
            w.minc().set_bit();
            w.circ().set_bit();
            w.dir().clear_bit();
            w.msize().bits(DMA_MEMORIA_16_BITS);
            w.psize().bits(DMA_PERIFERICO_32_BITS);
            w.pl().bits(DMA_PRIORIDAD_ALTA);
            w.en().set_bit();
            w
        });
    }
}

fn arrancar_adc(adc: &stm32g474::ADC1) {
    adc.cr().modify(|_, w| w.adstart().set_bit());
}

fn leer_buffer_adc(adc_buffer: &[u16; N_MUESTRAS]) -> &[u16] {
    unsafe { core::slice::from_raw_parts(adc_buffer.as_ptr(), N_MUESTRAS) }
}

#[entry]
fn main() -> ! {
    defmt::println!("Iniciando STM32G474...");
    let dp = stm32g474::Peripherals::take().unwrap();

    // TIM6 conserva el arranque original; los perfiles se aplican al pulsar el boton.
    let mut perfil_actual = PERFIL_INICIAL;

    let rcc = dp.RCC;
    habilitar_relojes(&rcc);

    let gpioa = dp.GPIOA;
    configurar_gpioa(&gpioa);

    let tim6 = dp.TIM6;
    configurar_tim6(&tim6);

    let dac = dp.DAC1;
    configurar_dac(&dac);

    let dma1 = dp.DMA1;
    configurar_dma_dac(&dma1, &dac);

    let dmamux = dp.DMAMUX;
    configurar_dmamux(&dmamux);

    let adc = dp.ADC1;
    let adc_common = dp.ADC12_COMMON;
    configurar_adc(&adc, &adc_common);

    let adc_buffer = singleton!(: [u16; N_MUESTRAS] = [0; N_MUESTRAS]).unwrap();
    configurar_dma_adc(&dma1, &adc, adc_buffer);

    let gpioc: GpioC = dp.GPIOC;
    configurar_boton(&gpioc);

    arrancar_adc(&adc);
    defmt::println!("ADC en marcha");

    loop {
        cortex_m::asm::delay(DELAY_BUCLE_PRINCIPAL);

        let boton_pulsado = gpioc.idr().read().idr13().bit_is_set();

        if boton_pulsado {
            perfil_actual = (perfil_actual + 1) % PERFILES.len();
            aplicar_perfil_frecuencia(&tim6, &PERFILES[perfil_actual]);
        }

        let lectura_adc = leer_buffer_adc(adc_buffer);
        defmt::println!(
            "Frecuencia {:?}Hz: {:?}",
            PERFILES[perfil_actual].frecuencia_hz,
            lectura_adc
        );
    }
}