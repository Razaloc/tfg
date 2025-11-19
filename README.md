# Generación y Adquisición de Señal Senoidal en STM32G474RE

Este proyecto implementa un sistema embebido en **Rust** para un microcontrolador **STM32G474RE** capaz de:

* Generar una **onda senoidal** mediante el **DAC1**.
* Enviarla de forma estable usando **DMA en modo circular**.
* Disparar el sistema mediante un **timer (TIM6)**.
* Leer la señal resultante tras atravesar un medio (por ejemplo, **tejido biológico**) usando **ADC1 + DMA**.
* Procesar las muestras en tiempo real.

El objetivo final es emplear esta plataforma como base para **adquisición biomédica** en experimentos de propagación de señales a través de tejidos, con aplicaciones potenciales en monitorización, espectroscopía eléctrica y análisis de impedancia.

---

## ✨ Características principales

* **No-Std + Runtime minimal** (`no_std`, `no_main`).
* **Rust embedded** usando:

  * `cortex-m`
  * `cortex-m-rt`
  * `stm32g4`
  * `defmt`
  * `panic-probe`
* **DAC → DMA (CH3)** para reproducir una tabla senoidal de 32 muestras.
* **TIM6** genera el *trigger* a 10 kHz para el DAC y ADC.
* **ADC1** configurado con:

  * Calibración interna
  * Muestreo canal PA0 (ADC1_IN1)
  * Conversión externa ligada a TIM6
  * DMA (CH1) en modo circular
* **DMAMUX** correctamente configurado para enlazar DAC1 y ADC1 con DMA.
* **Procesamiento básico**: cálculo de promedio, comparación con la referencia DAC.

---

## 🧠 Arquitectura del sistema

```text
     ┌───────────┐     DMA CH3 (tabla seno)
     │  SINE      │==============================┐
     │  TABLE     │                              │
     └───────────┘                              ▼
                                            ┌────────┐
                                            │  DAC1  │─── Señal → Tejido → Sensor
                                            └────────┘
                                                 ▲
                        TIM6_TRGO (10 kHz)       │
                                                 │
                                           ┌────────┐
                                           │ TIM6   │
                                           └────────┘
                                                 │
                                                 ▼
                                            ┌────────┐
                                            │ ADC1   │─── DMA CH1 → Buffer ADC
                                            └────────┘
```

---

## ⚙️ Flujo de inicialización

1. **RCC** habilita relojes de GPIO, DAC, ADC, DMA y TIM6.
2. **GPIOA** se configura en modo analógico:

   * PA4 → DAC1_OUT1
   * PA0 → ADC1_IN1
3. **TIM6** se configura para generar un *trigger* a **10 kHz**.
4. **DAC1 + DMA** cargan repetidamente la tabla senoidal.
5. **DMAMUX** enlaza correctamente DAC1_CH1 y ADC1.
6. **ADC1** se calibra, configura y se activa en modo con *external trigger*.
7. **DMA1 CH1** almacena en un buffer de 32 muestras.
8. Bucle principal imprime las muestras y compara desviaciones.

---

## 📡 Tabla senoidal (32 muestras)

Se utiliza una tabla precomputada de 12 bits centrada en 2048.

```rust
const SINE_TABLE: [u16; 32] = [...];
```

Esto permite generar una señal periódica estable usando el DAC + DMA.

---

## 🧪 Uso previsto

Este firmware está diseñado para:

* Experimentos de propagación en tejido
* Estudios de impedancia eléctrica
* Sensado biomédico experimental
* Prototipos de instrumentación en Rust

Puedes conectar:

* **PA4** → a un transductor, electrodo o actuador
* **PA0** → al punto donde se desea leer la señal atenuada/filtrada por el tejido

---

## 🧰 Requisitos

* Placa **STM32G474RE Nucleo** o compatible
* Toolchain Rust para embedded (`thumbv7em-none-eabihf`)
* `probe-rs` para flasheo y *debug*

Instalación rápida:

```bash
rustup target add thumbv7em-none-eabihf
cargo install probe-rs --locked
```

---

## ▶️ Compilar y cargar

```bash
cargo build --release
probe-rs run --chip STM32G474RETx target/thumbv7em-none-eabihf/release/firmware
```

Para logs con `defmt`:

```bash
probe-rs attach --defmt
```

---

## 🔍 Trabajo futuro

* Filtrado digital (FIR/IIR) en el microcontrolador
* FFT o análisis espectral en tiempo real
* Modulación de frecuencia o amplitud
* Interfaz USB para streaming de datos
* Integración con sensores biomédicos reales

---

## 📄 Licencia

MIT — libre uso para proyectos educativos, biomédicos o experimentales.

---

Si deseas ampliar el README con gráficos, diagramas fancier, instrucciones para hardware o documentación interna del código, puedo generarlos.
