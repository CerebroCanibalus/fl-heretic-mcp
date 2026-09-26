//! `daw_master` — medir, ajustar y masterizar, con la API real de Reaper.
//!
//! # De donde sale
//!
//! El doc de REAPER v7.80, no la imaginacion. Dos funciones que el bridge no
//! toca y que son justo el núcleo del masterizado:
//!
//! ```text
//! number reaper.CalculateNormalization(PCM_source source, integer normalizeTo,
//!        number normalizeTarget, number normalizeStart, number normalizeEnd)
//!   normalizeTo: 0=LUFS-I  1=RMS-I  2=pico  3=true pico
//!   normalizeTarget: dBFS objetivo
//!
//! number reaper.Track_GetPeakHoldDB(MediaTrack track, integer channel, boolean clear)
//!   metro en dB*0.01
//! ```
//!
//! # El objetivo cero, y el error que casi se cuela
//!
//! `CalculateNormalization` no devuelve "la sonoridad": devuelve **el factor
//! de ganancia lineal** que hay que aplicar para llegar a un objetivo. Es un
//! multiplicador, no decibelios.
//!
//! La conversion correcta es:
//!
//! ```text
//! medido_dB = objetivo_dB - 20 * log10(retorno)
//! ```
//!
//! Con objetivo `0` queda `medido_dB = -20 * log10(retorno)`.
//!
//! Esto se dio por bueno un rato entero y salio mal: leyendo el retorno como si
//! fueran dB, un WAV de -20 dBFS se informaba como -10 dBFS. Lo que de verdad
//! lo cazo fue el fixture con respuesta conocida —el WAV cuyo pico se mide a
//! mano con el modulo `wave` de Python—, no un test. Un test que comprobara
//! "devuelve un numero" habria pasado igual.
//!
//! # Lo que esta tool NO hace
//!
//! - **No renderiza.** Renderizar masteriza el archivo y escribe en disco.
//!   Es un efecto irreversible, asi que no se cuela en una tool de "medir y
//!   ajustar": seria cambiar el motor con el deposito lleno.
//! - **No toca preferencias.** La via alternativa (`RENDER_STATS` + el
//!   boton "Stats/Charts" de Preferencias) abre un dialogo modal en Reaper, y
//!   eso congela el DAW. Medimos con la API, sin dialogos.
//! - **No normaliza a ciegas.** Exige `target_lufs` explicito. Normalizar a
//!   -14 LUFS una pista que deberia estar a -20 es un error, no un detalle.

use std::path::PathBuf;

use flojo_mcp::prelude::*;
use serde_json::{json, Value};

use crate::debug;

/// Payload Lua: mide las cuatro magnitudes de un item.
///
/// `CalculateNormalization` con objetivo 0 devuelve el ajuste, y el ajuste es
/// la medida con el signo cambiado. Por eso `-norm(0)`.
fn payload_medir(track: usize, item: usize) -> String {
    format!(
        r#"
local tr = reaper.GetTrack(0, {track})
if not tr then return {{ error = "no hay pista {track}" }} end
local it = reaper.GetTrackMediaItem(tr, {item})
if not it then return {{ error = "la pista {track} no tiene item {item}" }} end
local take = reaper.GetActiveTake(it)
if not take then return {{ error = "el item {item} no tiene take" }} end
local src = reaper.GetMediaItemTake_Source(take)
if not src then return {{ error = "el take no tiene source" }} end

-- OJO: `CalculateNormalization` devuelve un FACTOR DE GANANCIA LINEAL, no
-- decibelios. Costo una sesion entera decreerlo, y un fixture con respuesta
-- conocida lo cazo a la primera.
--
-- La prueba: para el WAV de -20 dBFS devuelve 9.999, y 20*log10(9.999) es
-- 20.00 clavado. Variando el objetivo a -6 y +6 devuelve 5.012 y 19.951, y
-- -6 - 20*log10(5.012) y 6 - 20*log10(19.951) dan ambos -20.00. O sea:
--
--     medido_dB = objetivo_dB - 20 * log10(retorno)
--
-- Si se devuelve el valor tal cual y se le pone "dB" delante, todo queda
-- desplazado de forma systematica y parece una medicion.
-- Y `math.log10` NO EXISTE en el Lua de Reaper: devuelve nil y el payload
-- revienta con "attempt to call a nil value (field 'log10')". Comprobado:
-- `math.log` si esta, `math.log10` no. De ahi la division.
local LN10 = math.log(10)
local function norm(to)
  local ok, f = pcall(reaper.CalculateNormalization, src, to, 0, 0, 0)
  if not ok or not f or f <= 0 then return nil end
  return 0 - 20 * (math.log(f) / LN10)   -- objetivo 0
end

return {{
  lufs_i       = norm(0),
  rms_i        = norm(1),
  pico_dbfs    = norm(2),
  true_pico_dbfs = norm(3),
  -- del item y de la pista, para saber que se esta midiendo
  pos_s   = reaper.GetMediaItemInfo_Value(it, "D_POSITION"),
  len_s   = reaper.GetMediaItemInfo_Value(it, "D_LENGTH"),
  vol_pista = reaper.GetMediaTrackInfo_Value(tr, "D_VOL"),
  pico_metro = reaper.Track_GetPeakHoldDB(tr, 0, false) * 0.01,
}}
"#
    )
}

/// Genera las señales de prueba y devuelve la ruta de la carpeta.
fn pruebas_dir() -> std::result::Result<PathBuf, ToolError> {
    let base = std::env::var("TEMP")
        .map_err(|_| ToolError::internal("no encuentro %TEMP%"))?;
    Ok(PathBuf::from(base).join("daw_heretic_wavs"))
}

/// Measure, adjust and master audio, with Reaper's real metering API.
///
/// op:
/// - `probes` — the test signals this tool can generate, with the expected
///   relations between them. No DAW needed; use it to check the tool works.
/// - `make_probes` — writes those WAVs to disk and returns the paths.
/// - `import` — insert an audio file into the project on a new track.
/// - `measure` — LUFS-I, RMS-I, peak and true peak of an item, via
///   `CalculateNormalization`. This is the measurement a mastering decision
///   should be based on, not the fader.
/// - `normalize` — measure and then move the track fader to hit `target_lufs`,
///   in one step. Reports the gain it applied. **You must pass
///   `target_lufs`**: normalizing to a default is how a mix ends up at
///   -14 LUFS when it was supposed to be at -20.
///
/// Loudness units: LUFS for `lufs_i`, dBFS for the rest. All numbers are
/// negative, and a smaller number is quieter. Reaper's own convention, not a
/// normalisation to 0..1 like the FX parameters use.
#[tool(description = "Measure, adjust and master audio using Reaper's real metering API. op=make_probes writes test WAVs whose expected loudness relations are known (so the tool itself can be checked); op=import inserts an audio file on a new track; op=measure returns LUFS-I, RMS-I, peak and true peak of an item via CalculateNormalization - that is what a mastering decision should be based on, not the fader; op=normalize measures and then moves the track fader to hit target_lufs in one step, and requires you to pass it explicitly. All values are negative and smaller means quieter.")]
pub async fn daw_master(
    op: String,
    path: Option<String>,
    track: Option<usize>,
    item: Option<usize>,
    target_lufs: Option<f64>,
) -> std::result::Result<Value, ToolError> {
    let t = track.unwrap_or(0);
    let i = item.unwrap_or(0);

    match op.as_str() {
        "probes" | "sondas" => Ok(json!({
            "para_que": "señales con respuesta conocida, para comprobar que la \
                         medición mide y no devuelve un numero decorativo",
            "senales": heretic_daw::SENALES.iter().map(|s| json!({
                "fichero": s.nombre,
                "que_es": s.descripcion,
                "dbfs_nominal": s.dbfs,
                "recortado": s.recortar,
            })).collect::<Vec<_>>(),
            "relaciones_a_comprobar": {
                "sine_1k_3db sobre sine_1k_20db": "+17 dB (fijo por generacion)",
                "clipped": "pico 0 dBTP pero LUFS mucho menor que el pico: \
                            es el recorte, y es la cancion que hay que remasterizar",
            },
        })),

        "make_probes" | "generar" => {
            let dir = pruebas_dir()?;
            let hechos = heretic_daw::wav::escribir_todas(&dir).map_err(|e| {
                ToolError::internal(format!("no pude escribir en {}: {e}", dir.display()))
            })?;
            Ok(json!({
                "carpeta": dir.display().to_string(),
                "ficheros": hechos.iter().map(|(n, d, p)| json!({
                    "nombre": n, "que_es": d, "ruta": p.display().to_string()
                })).collect::<Vec<_>>(),
                "siguiente_paso": "daw_master op=import path=<uno de ellos>, \
                                   y luego op=measure.",
            }))
        }

        "import" | "importar" => {
            let ruta = path.ok_or_else(|| {
                ToolError::invalid_params(
                    "op=import necesita `path` con el fichero de audio. \
                     Usa daw_master op=make_probes para tener pruebas que importar.",
                )
            })?;
            let p = PathBuf::from(&ruta);
            if !p.exists() {
                return Err(ToolError::invalid_params(format!(
                    "no existe el fichero {ruta}. Si es una ruta de Windows con \
                     barra invertida, pasala con barras dobles: en JSON un \
                     backslash suelto rompe el valor entero."
                )));
            }
            // Importar por la API cruda. Sin `BrowseForOpenFiles`: eso abre
            // un selector de ficheros MODAL y con el DAW congelado, que es
            // justo el fallo que esta tool viene a evitar. El MCP ya tiene la
            // ruta; no hay nada que preguntar.
            let codigo = format!(
                r#"
local ruta = [==[{ruta}]==]
local idx = reaper.CountTracks(0)
reaper.InsertTrackAtIndex(idx, true)
local tr = reaper.GetTrack(0, idx)
reaper.SetOnlyTrackSelected(tr)
-- OJO: `InsertMedia` devuelve un ENTERO (si tuvo exito), no el item. Leyendo
-- el catalogo: "integer reaper.InsertMedia(string file, integer mode)".
-- Pasarselo a GetMediaItemInfo_Value da "bad argument" y el item nunca se
-- localiza, asi que el item se busca por indice en la pista.
local ok = reaper.InsertMedia(ruta, 0)
if not ok or ok == 0 then return {{ error = "InsertMedia no inserto nada" }} end
reaper.UpdateArrange()
local n = reaper.CountTrackMediaItems(tr)
if n == 0 then return {{ error = "la pista quedo vacia tras importar" }} end
local it = reaper.GetTrackMediaItem(tr, n - 1)
return {{
  pista = idx,
  item = n - 1,
  duracion_s = reaper.GetMediaItemInfo_Value(it, "D_LENGTH"),
}}
"#
            );
            let r = debug::lua(&codigo).await?;
            if let Some(e) = r.get("error").and_then(|v| v.as_str()) {
                return Err(ToolError::internal(format!("Reaper no pudo importar: {e}")));
            }
            let v = r.get("value").cloned().unwrap_or(Value::Null);
            // Se devuelve DONDE ha quedado, no solo que ha funcionado: el paso
            // siguiente es `measure track=<n> item=<n>` y adivinarlo es un
            // viaje de ida y vuelta al DAW por nada.
            let pista = v.get("pista").and_then(|x| x.as_i64()).unwrap_or(0);
            let item = v.get("item").and_then(|x| x.as_i64()).unwrap_or(0);
            Ok(json!({
                "importado": true,
                "fichero": ruta,
                "donde": v,
                "siguiente_paso": format!("daw_master op=measure track={pista} item={item}"),
            }))
        }

        "measure" | "medir" => {
            let r = debug::lua(&payload_medir(t, i)).await?;
            // `CalculateNormalization` hace un analisis en seco: puede tardar.
            // Si el payload devuelve `error`, es un fallo de ruteo, no de medicion.
            if let Some(e) = r.get("error").and_then(|v| v.as_str()) {
                return Err(ToolError::invalid_params(e.to_string()));
            }
            let val = r.get("value").cloned().unwrap_or(Value::Null);
            // El veredicto va junto a los numeros. Darle a un agente cuatro
            // cifras y nada que concluir es devolverle el trabajo de
            // interpretarlas, que es justo lo que esta tool existe para evitar.
            let pico = val.get("pico_dbfs").and_then(|v| v.as_f64());
            let rms = val.get("rms_i").and_then(|v| v.as_f64());
            let (estado, porque) = match (pico, rms) {
                (Some(p), Some(r)) => heretic_daw::wav::diagnostico_de_cresta(p, r),
                _ => ("sin_datos", "Reaper no devolvio pico ni RMS".to_string()),
            };
            Ok(json!({
                "track": t,
                "item": i,
                "medida": val,
                "diagnostico": { "estado": estado, "porque": porque },
                "como_leerlo": {
                    "lufs_i": "sonoridad integrada. -14 es lo que espera el \
                               streaming, -23 lo que piden las emisoras. No hay \
                               un valor correcto unico: depende de donde va.",
                    "factor_de_cresta": "pico menos RMS. Un seno sano da 3.01 dB. \
                                         Por debajo de 2 dB hay crestas aplastadas: \
                                         o el master está muy fuerte, o el archivo \
                                         vino recortado de casa.",
                    "true_pico_dbfs": "por encima de -1 dBTP hay recorte al \
                                       codificar; por debajo de -9 sobra margen.",
                },
            }))
        }

        "normalize" | "normalizar" => {
            let objetivo = target_lufs.ok_or_else(|| {
                ToolError::invalid_params(
                    "op=normalize necesita `target_lufs` explicito. Sin el, \
                     normalizar a un valor por defecto es como una mezcla acaba \
                     en -14 LUFS sin que nadie lo decidiera. Si solo quieres \
                     saber como suena de fuerte, usa op=measure.",
                )
            })?;
            let m = debug::lua(&payload_medir(t, i)).await?;
            let val = m.get("value").cloned().unwrap_or(Value::Null);
            if let Some(e) = val.get("error").and_then(|v| v.as_str()) {
                return Err(ToolError::invalid_params(e.to_string()));
            }
            let actual = val
                .get("lufs_i")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| ToolError::internal("Reaper no devolvio lufs_i"))?;
            let delta = objetivo - actual;
            // Mover el fader: `D_VOL` es la escala de Reaper, 0.5 = -6 dB, y
            // linear, o sea 20*log10(vol). Para sumar `delta` dB hay que
            // multiplicar por 10^(delta/20).
            let codigo = format!(
                r#"
local tr = reaper.GetTrack(0, {t})
if not tr then return {{ error = "no hay pista {t}" }} end
local antes = reaper.GetMediaTrackInfo_Value(tr, "D_VOL")
local nuevo = antes * math.pow(10, {delta} / 20)
if nuevo < 0 then nuevo = 0 end
if nuevo > 2 then nuevo = 2 end
reaper.SetMediaTrackInfo_Value(tr, "D_VOL", nuevo)
reaper.UpdateArrange()
return {{ antes = antes, nuevo = nuevo, dB = 20 * math.log10(nuevo) }}
"#
            );
            let ap = debug::lua(&codigo).await?;
            let av = ap.get("value").cloned().unwrap_or(Value::Null);
            Ok(json!({
                "track": t,
                "objetivo_lufs": objetivo,
                "medido_lufs": actual,
                "delta_db": delta,
                "fader": av,
                "aviso": "el delta se aplico al fader, no a los plugs. Si hay \
                          un compresor o un limitador despues en la cadena, el \
                          resultado final no sera este: vuelve a medir.",
            }))
        }

        otro => Err(ToolError::invalid_params(format!(
            "op desconocido: {otro}. Validas: probes, make_probes, import, measure, normalize"
        ))),
    }
}
