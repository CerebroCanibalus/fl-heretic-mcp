//! Generador de WAV de prueba.
//!
//! ## Por qué existe
//!
//! Una tool de master que no se puede verificar no sirve. Y no se puede
//! verificar mirando el número que devuelve Reaper y "`confia en mi`": hay que
//! comprobarlo contra algo.
//!
//! Estos ficheros son las respuestas:
//!
//! | fichero | qué es | qué DEBE dar la medición |
//! |---|---|---|
//! | `sine_1k_20db.wav` | 1 kHz a -20 dBFS | LUFS-I cerca de -20, pico -20 dBTP |
//! | `sine_1k_3db.wav` | 1 kHz a -3 dBFS | ~17 dB por encima del anterior |
//! | `clipped.wav` | 1 kHz a +6 dBFS recortado duro | pico 0 dBTP, y **factor de cresta de 1.07 dB en vez de 3.01**: las crestas están aplastadas |
//! | `noise.wav` | ruido blanco a -18 dBFS | RMS cercano a -18, LUFS menor que el pico |
//!
//! ## El factor de cresta, y por qué importa `clipped.wav`
//!
//! Escribí aquí que el fichero recortado tendría "mucha menos sonoridad que su
//! pico", y es al revés. Medido: el seno limpio tiene un factor de cresta de
//! 3.01 dB (RMS 3 dB por debajo del pico, que es lo de toda onda senoidal) y
//! el recortado solo 1.07 dB.
//!
//! Es decir: **recortar aplasta las crestas y acerca el RMS al pico**. Y eso es
//! exactamente la firma del recorte, medible con dos números que ya tenía:
//! `pico - rms`. Un masterizer que solo mira el pico no ve nada raro â€” 0 dBTP
//! es "un pico alto"â€”, pero un factor de cresta de 1 dB sobre 3 dB es un
//! archivo destrozado.
//!
//! La lección se queda en el repo porque es del tipo de expectativa que hace
//! que un test pase sin comprobar nada.
//!
//! ## Formato
//!
//! WAV PCM 16 bits little-endian, mono, 44100 Hz. Cabecera RIFF estandar, la
//! que aceptan Reaper, ffmpeg y todo lo demas. Sin dependencias: se escriben a
//! mano los 44 bytes.

use std::io;
use std::path::Path;

const TASA: u32 = 44100;

/// Amplitud lineal -> dBFS. `0.0` es silencio.
fn lineal_a_db(a: f32) -> f32 {
    if a <= 0.0 {
        f32::NEG_INFINITY
    } else {
        20.0 * a.log10()
    }
}

/// dbFS -> amplitud lineal.
fn db_a_lineal(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// La onda que se va a escribir, con su nombre y lo que se espera de ella.
pub struct Senal {
    pub nombre: &'static str,
    pub descripcion: &'static str,
    /// dbFS nominal, antes de recortar.
    pub dbfs: f32,
    /// Si es `Some`, se recorta duro a ±1.0 (el "clipped" del masterizado).
    pub recortar: bool,
    /// `true` = ruido blanco; `false` = seno.
    pub ruido: bool,
    pub segundos: f32,
}

pub const SENALES: &[Senal] = &[
    Senal {
        nombre: "sine_1k_20db.wav",
        descripcion: "seno 1 kHz a -20 dBFS: la referencia tranquilo",
        dbfs: -20.0,
        recortar: false,
        ruido: false,
        segundos: 3.0,
    },
    Senal {
        nombre: "sine_1k_3db.wav",
        descripcion: "seno 1 kHz a -3 dBFS: 17 dB por encima, casi al limite",
        dbfs: -3.0,
        recortar: false,
        ruido: false,
        segundos: 3.0,
    },
    Senal {
        nombre: "clipped.wav",
        descripcion: "seno 1 kHz a +6 dBFS recortado duro: pico 0 dBTP y crestas aplastadas",
        dbfs: 6.0,
        recortar: true,
        ruido: false,
        segundos: 3.0,
    },
    Senal {
        nombre: "noise.wav",
        descripcion: "ruido blanco a -18 dBFS",
        dbfs: -18.0,
        recortar: false,
        ruido: true,
        segundos: 3.0,
    },
];

/// LCG determinista: un WAV de prueba tiene que ser **el mismo** en cada
/// ejecución, o dos mediciones no son comparables.
fn ruido_uniforme(semilla: &mut u64) -> f32 {
    *semilla = semilla.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    // 24 bits del estado alto, mapeados a [-1, 1)
    let v = ((*semilla >> 40) as f32) / 8388608.0 - 1.0;
    v
}

/// Genera las muestras en [-1, 1] de una señal.
pub fn muestras(s: &Senal) -> Vec<f32> {
    let n = (TASA as f32 * s.segundos) as usize;
    let amp = db_a_lineal(s.dbfs);
    let mut semilla = 0x5EED_1234_ABCD_0001u64;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / TASA as f32;
        let mut v = if s.ruido {
            ruido_uniforme(&mut semilla)
        } else {
            (2.0 * std::f32::consts::PI * 1000.0 * t).sin()
        } * amp;
        if s.recortar {
            v = v.clamp(-1.0, 1.0);
        }
        out.push(v);
    }
    out
}

/// Envuelve muestras PCM 16 bits en un WAV mono.
pub fn a_wav(muestras: &[f32]) -> Vec<u8> {
    let datos: Vec<u8> = muestras
        .iter()
        .flat_map(|v| {
            let i = (v.clamp(-1.0, 1.0) * 32767.0).round() as i16;
            i.to_le_bytes()
        })
        .collect();
    let mut w = Vec::with_capacity(44 + datos.len());
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&((36 + datos.len()) as u32).to_le_bytes());
    w.extend_from_slice(b"WAVE");
    w.extend_from_slice(b"fmt ");
    w.extend_from_slice(&16u32.to_le_bytes()); // tamaño del bloque fmt
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM sin comprimir
    w.extend_from_slice(&1u16.to_le_bytes()); // mono
    w.extend_from_slice(&TASA.to_le_bytes());
    w.extend_from_slice(&(TASA * 2).to_le_bytes()); // bytes por segundo
    w.extend_from_slice(&2u16.to_le_bytes()); // alineacion
    w.extend_from_slice(&16u16.to_le_bytes()); // bits
    w.extend_from_slice(b"data");
    w.extend_from_slice(&(datos.len() as u32).to_le_bytes());
    w.extend_from_slice(&datos);
    w
}

/// Escribe una señal en `dir`. Devuelve la ruta.
pub fn escribir(dir: &Path, s: &Senal) -> io::Result<std::path::PathBuf> {
    std::fs::create_dir_all(dir)?;
    let p = dir.join(s.nombre);
    std::fs::write(&p, a_wav(&muestras(s)))?;
    Ok(p)
}

/// Escribe las cuatro señales de prueba.
pub fn escribir_todas(dir: &Path) -> io::Result<Vec<(String, String, std::path::PathBuf)>> {
    let mut out = Vec::new();
    for s in SENALES {
        let p = escribir(dir, s)?;
        out.push((s.nombre.to_string(), s.descripcion.to_string(), p));
    }
    Ok(out)
}

/// Factor de cresta en dB: `pico - rms`.
///
/// Es la medida mas util para decidir si un archivo necesita masterizado, y la
/// que se puede construir con dos numeros que `CalculateNormalization` ya
/// devuelve. Un material sano tiene el factor de cresta de su propia forma de
/// onda: 3.01 dB para un seno, ~10-12 dB para musica con transitorios. Un
/// masterizado fuerte lo baja a proposito. **Un factor bajo con picos a 0 dBTP
/// es un archivo recortado**, y ahi si hay que.masterizar.
pub fn factor_de_cresta(pico_dbfs: f64, rms_db: f64) -> f64 {
    pico_dbfs - rms_db
}

/// Interpretacion de un par (pico, RMS) medido, con la logica escrita.
pub fn diagnostico_de_cresta(pico_dbfs: f64, rms_db: f64) -> (&'static str, String) {
    let c = factor_de_cresta(pico_dbfs, rms_db);
    let veredicto = if pico_dbfs > -0.5 {
        if c < 2.0 {
            ("recortado", format!(
                "pico en {pico_dbfs:.1} dBTP y factor de cresta de solo {c:.1} dB. \
                 Es un archivo recortado: las crestas están aplastadas y al \
                 codificar se oirá distorsion. Es el caso que hay que \
                 remasterizar."))
        } else {
            ("sin_margin", format!(
                "pico en {pico_dbfs:.1} dBTP: no queda margen, al codificar \
                 se recorta. Baja el pico a -1 dBTP o menos."))
        }
    } else if c < 2.0 {
        ("comprimido_al_maximo", format!(
            "pico en {pico_dbfs:.1} dBTP con factor de cresta de {c:.1} dB. \
             Suena aplastado: la compresion es extrema o ya se masterizo."))
    } else {
        ("sano", format!(
            "pico en {pico_dbfs:.1} dBTP y factor de cresta de {c:.1} dB."))
    };
    veredicto
}

/// Lo que se espera de medir un WAV, para comparar con lo que diga Reaper.
///
/// Deliberadamente **relativo**: la comprobacion dura no es "este fichero mide
/// -20 LUFS" (que dependeria de la curva K y de la implementacion), sino
/// "este fichero mide 17 dB mas que el otro". Esa diferencia si la fija la
/// generacion, y si el master esta bien, se cumple.
pub fn relacione_esperada(a: &str, b: &str) -> Option<f32> {
    match (a, b) {
        ("sine_1k_3db.wav", "sine_1k_20db.wav") => Some(17.0),
        ("sine_1k_20db.wav", "sine_1k_3db.wav") => Some(-17.0),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pico(nombre: &str) -> f32 {
        let s = SENALES.iter().find(|s| s.nombre == nombre).unwrap();
        muestras(s).iter().fold(0.0f32, |a, &b| a.max(b.abs()))
    }

    #[test]
    fn la_cabecera_wav_es_una_de_44_bytes() {
        // Si esto no son 44, ningun lector lo abre y el fallo aparece lejos de
        // la causa.
        let w = a_wav(&[0.0, 0.5, -0.5]);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(&w[8..12], b"WAVE");
        assert_eq!(&w[12..16], b"fmt ");
        assert_eq!(&w[36..40], b"data");
        let tam: u32 = u32::from_le_bytes(w[40..44].try_into().unwrap());
        assert_eq!(tam as usize, w.len() - 44);
    }

    #[test]
    fn el_seno_quieto_tiene_el_pico_que_dice() {
        // -20 dBFS -> 0.1 exacto. Margen de 1 LSB de 16 bits.
        let p = pico("sine_1k_20db.wav");
        assert!((p - 0.1).abs() < 0.001, "pico {p}, se esperaba 0.1");
        assert!((lineal_a_db(p) - -20.0).abs() < 0.1);
    }

    #[test]
    fn el_recortado_pica_en_uno_y_no_en_1_26() {
        // +6 dBFS es 1.995 de amplitud: sin recortar pasaria de 1.0. Con
        // recorte, el pico tiene que ser 1.0 clavado.
        let p = pico("clipped.wav");
        assert!(p <= 1.0, "el recorte no ha funcionado: pico {p}");
        assert!((p - 1.0).abs() < 0.001, "pico {p}, se esperaba 1.0");
    }

    #[test]
    fn el_ruido_no_llega_al_pico_nominal() {
        // El pico de un ruido blanco con semilla fija, medido y no supuesto.
        let p = pico("noise.wav");
        assert!(p > 0.02 && p <= 0.2, "pico de ruido inesperado: {p}");
    }

    #[test]
    fn dos_generadas_dan_el_mismo_fichero() {
        // Si el ruido cambia entre ejecuciones, dos mediciones dejan de ser
        // comparables y el test del master no significa nada.
        let a = a_wav(&muestras(&SENALES[3]));
        let b = a_wav(&muestras(&SENALES[3]));
        assert_eq!(a, b);
    }

    #[test]
    fn el_factor_de_cresta_distingue_lo_recortado_de_lo_sano() {
        // Medidos con el modulo `wave` de Python sobre los ficheros reales:
        // seno limpio pico -20 / RMS -23.01; recortado pico 0 / RMS -1.07.
        let (v, _) = diagnostico_de_cresta(-20.0, -23.01);
        assert_eq!(v, "sano");
        let (v, m) = diagnostico_de_cresta(-0.0, -1.07);
        assert_eq!(v, "recortado", "{m}");
        assert!(m.contains("remasterizar"), "{m}");
        // Pico a 0 pero con crestas sanas: no es recorte, es falta de margen.
        assert_eq!(diagnostico_de_cresta(0.0, -12.0).0, "sin_margin");
    }

    #[test]
    fn el_factor_de_cresta_es_la_resta_y_no_un_adorno() {
        assert!((factor_de_cresta(-20.0, -23.01) - 3.01).abs() < 0.01);
        assert!((factor_de_cresta(0.0, -1.07) - 1.07).abs() < 0.01);
        // Un seno son 3.01 dB de diferencia entre pico y RMS: el valor teorico.
        assert!((factor_de_cresta(0.0, -3.01) - 3.01).abs() < 0.01);
    }

    #[test]
    fn la_relacion_de_17_db_se_va_a_verificar_contra_reaper() {
        assert_eq!(relacione_esperada("sine_1k_3db.wav", "sine_1k_20db.wav"), Some(17.0));
        assert_eq!(relacione_esperada("clipped.wav", "sine_1k_20db.wav"), None);
    }
}

