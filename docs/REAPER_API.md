# API de Reaper (ReaScript) â€” referencia compacta

> **GENERADO. No editar a mano.** Sale de `reference/reaper/reascripthelp.html` (el doc oficial de REAPER v7.80, 730 funciones) via `tools/gen_api_docs.py` + `tools/gen_api_md.py`.
> Editarlo a mano es la forma mas rapida de que este fichero mienta.

```
# regenerar (una vez descargado el HTML)
curl -o reference/reaper/reascripthelp.html \
  https://www.reaper.fm/sdk/reascript/reascripthelp.html
python tools/gen_api_docs.py    # HTML -> data/reaper-api.json
python tools/gen_api_md.py      # JSON -> este .md
```

## 1. Cuanto de esto usan las tools hoy

| | funciones |
|---|---|
| API real de REAPER v7.80 | **730** |
| llama el bridge Lua (`reaper.X(...)`) | 161 |
| **inalcanzables hoy** | **569** |

Las 569 inalcanzables no son un defecto del bridge: son la API que este no envuelve. Es exactamente donde estan las funciones de analisis y mastering (seccion 4). El escape es `script_run_start` + Lua propia.

## 2. Las trampas (las que ya han costado un bug cada una)

| trampa | real | por duele |
|---|---|---|
| el bridge usa guion bajo, no punto | `track_create` | `track.create` -> `Unknown command` sin decir cual era el bueno |
| los params van en `snake_case` | `track_index`, `fx_name` | `trackIndex` -> `Missing parameter` |
| nota MIDI usa `end`, no `length` | `{"start":0,"end":0.5}` | el bridge ignora en silencio la nota si falta `end` |
| velocity va en 0-127, no 0-1 | `vel = 112` | 1.0 es inaudible |
| hay prefijos duplicados en el catalogo | `fx.fx_add` -> `fx_add` | poner `fx_fx_add` no existe |
| prefijo duplicado tambien en midi | `midi.midi_insert_note` -> `midi_insert_note` | `midi_midi_insert_note` no existe |
| posiciones MIDI en BEATS, no segundos | `start = 4` es el compas 2 | se escribe todo aplastado al principio |

Regla que sustituye a acordarse de todo esto: **si no esta en el catalogo, no se inventa**. `daw_catalog` y `daw_api` lo dicen, y el error de un op desconocido lista los parecidos.

## 3. Como se llega a las 569 funciones sin cubrir

`script_run_start` ejecuta un `.lua` propio. Ahi cabe cualquier `reaper.X(...)`, que es como se llega a la API sin envolver. El problema de siempre son los punteros opacos (`MediaTrack`, `PCM_source`), que no viajan en JSON: hay que resolverlos dentro del Lua a partir de indices.

| lo que pide el agente | como se resuelve en Lua |
|---|---|
| `track: 0` | `reaper.GetTrack(reaper.GetActiveProject(), 0)` |
| `item: 0` en `track: 0` | `reaper.GetTrackMediaItem(tr, 0)` |
| `take: 0` de un item | `reaper.GetActiveTake(item)` |
| `source` de un take | `reaper.GetMediaItemTake_Source(take)` |
| `proj: 0` = proyecto actual | casi toda funcion acepta `0` |

## 4. Lo que sirve para componer y remasterizar

De aqui sale el diseno de `daw_music` y `daw_master`: cada funcion de esta tabla es una capacidad que el agente no tiene hoy. Las marcadas *(libre)* no las usa el bridge, o sea que hay que llamarlas por Lua.

### Medir sonoridad (LUFS / pico / RMS)

_Remasterizar empieza por medir. Sin esto el agente ajusta a ojo._

| funcion | libre | firma | que hace |
|---|---|---|---|
| `CalcMediaSrcLoudness` | si | `integer reaper.CalcMediaSrcLoudness(PCM_source mediasource)` | Calculates loudness statistics of media via dry run render. Statistics will be |
| `CalculateNormalization` | si | `number reaper.CalculateNormalization(PCM_source source, integer normalizeT` | Calculate normalize adjustment for source media. normalizeTo: 0=LUFS-I, 1=RMS- |
| `ClearPeakCache` | si | `reaper.ClearPeakCache()` | resets the global peak caches |
| `GetMediaItemTake_Peaks` | si | `integer reaper.GetMediaItemTake_Peaks(MediaItem_Take take, number peakrate` | Gets block of peak samples to buf. Note that the peak samples are interleaved, |
| `GetPeakFileName` | si | `string buf = GetPeakFileName(string fn)` | get the peak file name for a given file (can be either filename.reapeaks,or a  |
| `GetPeakFileNameEx` | si | `string buf = GetPeakFileNameEx(string fn, string buf, boolean forWrite)` | get the peak file name for a given file (can be either filename.reapeaks,or a  |
| `MediaExplorerGetLastPlayedFileInfo` | si | `boolean retval, string filename, integer filemode, number selstart, number` | Get information about the most recently previewed Media Explorer file. filenam |
| `PCM_Source_GetPeaks` | si | `integer reaper.PCM_Source_GetPeaks(PCM_source src, number peakrate, number` | Gets block of peak samples to buf. Note that the peak samples are interleaved, |
| `SetThemeColor` | si | `integer reaper.SetThemeColor(string ini_key, integer color, integer flags)` | Temporarily updates the theme color to the color specified (or the theme defau |
| `TrackFX_GetNamedConfigParm` | si | `boolean retval, string buf = TrackFX_GetNamedConfigParm(MediaTrack track, ` | gets plug-in specific named configuration value (returns true on success). Sup |
| `Track_GetPeakHoldDB` | si | `number reaper.Track_GetPeakHoldDB(MediaTrack track, integer channel, boole` | Returns meter hold state, in dB*0.01 (0 = +0dB, -0.01 = -1dB, 0.02 = +2dB, etc |
| `GetMediaTrackInfo_Value` |  | `number reaper.GetMediaTrackInfo_Value(MediaTrack tr, string parmname)` | Get track numerical-value attributes. B_MUTE : bool * : muted B_PHASE : bool * |
| `GetSetProjectInfo` |  | `number reaper.GetSetProjectInfo(ReaProject project, string desc, number va` | Get or set project information. RENDER_SETTINGS : (&(1/2)==0)=master mix, &1=s |
| `SetMediaTrackInfo_Value` |  | `boolean reaper.SetMediaTrackInfo_Value(MediaTrack tr, string parmname, num` | Set track numerical-value attributes. B_MUTE : bool * : muted B_PHASE : bool * |

_... y 1 mas; `daw_api` las encuentra por palabra._

### Leer y escribir buffers de audio

_Acceso a los samples de verdad: analisis de espectro y deteccion de clipping._

| funcion | libre | firma | que hace |
|---|---|---|---|
| `ApplyNudge` | si | `boolean reaper.ApplyNudge(ReaProject project, integer nudgeflag, integer n` | nudgeflag: &1=set to value (otherwise nudge by value), &2=snap nudgewhat: 0=po |
| `AudioAccessorStateChanged` | si | `boolean reaper.AudioAccessorStateChanged(AudioAccessor accessor)` | Returns true if the underlying samples (track or media item take) have changed |
| `AudioAccessorUpdate` | si | `reaper.AudioAccessorUpdate(AudioAccessor accessor)` | Force the accessor to reload its state from the underlying track or media item |
| `CreateTakeAudioAccessor` | si | `AudioAccessor reaper.CreateTakeAudioAccessor(MediaItem_Take take)` | Create an audio accessor object for this take. Must only call from the main th |
| `CreateTrackAudioAccessor` | si | `AudioAccessor reaper.CreateTrackAudioAccessor(MediaTrack track)` | Create an audio accessor object for this track. Must only call from the main t |
| `DestroyAudioAccessor` | si | `reaper.DestroyAudioAccessor(AudioAccessor accessor)` | Destroy an audio accessor. Must only call from the main thread. See CreateTake |
| `Envelope_Evaluate` | si | `integer retval, number value, number dVdS, number ddVdS, number dddVdS = E` | Get the effective envelope value at a given time position. samplesRequested is |
| `GetAudioAccessorEndTime` | si | `number reaper.GetAudioAccessorEndTime(AudioAccessor accessor)` | Get the end time of the audio that can be returned from this accessor. See Cre |
| `GetAudioAccessorHash` | si | `string hashNeed128 = GetAudioAccessorHash(AudioAccessor accessor, string h` | Deprecated. See AudioAccessorStateChanged instead. |
| `GetAudioAccessorSamples` | si | `integer reaper.GetAudioAccessorSamples(AudioAccessor accessor, integer sam` | Get a block of samples from the audio accessor. Samples are extracted immediat |
| `GetAudioAccessorStartTime` | si | `number reaper.GetAudioAccessorStartTime(AudioAccessor accessor)` | Get the start time of the audio that can be returned from this accessor. See C |
| `GetInputOutputLatency` | si | `integer inputlatency, integer outputLatency = GetInputOutputLatency()` | Gets the audio device input/output latency in samples |
| `GetItemEditingTime2` | si | `number, PCM_source which_item, integer flags = GetItemEditingTime2()` | returns time of relevant edit, set which_item to the pcm_source (if applicable |
| `GetMediaItemTake_Peaks` | si | `integer reaper.GetMediaItemTake_Peaks(MediaItem_Take take, number peakrate` | Gets block of peak samples to buf. Note that the peak samples are interleaved, |

_... y 17 mas; `daw_api` las encuentra por palabra._

### Notas, tono y afinacion

_Convierte 'Eb2' en el numero de nota correcto sin que el agente calcule._

| funcion | libre | firma | que hace |
|---|---|---|---|
| `GetTrackMIDINoteName` | si | `string reaper.GetTrackMIDINoteName(integer track, integer pitch, integer c` | see GetTrackMIDINoteNameEx |
| `GetTrackMIDINoteNameEx` | si | `string reaper.GetTrackMIDINoteNameEx(ReaProject proj, MediaTrack track, in` | Get note/CC name. pitch 128 for CC0 name, 129 for CC1 name, etc. See SetTrackM |
| `SetTrackMIDINoteName` | si | `boolean reaper.SetTrackMIDINoteName(integer track, integer pitch, integer ` | channel < 0 assigns these note names to all channels. |
| `SetTrackMIDINoteNameEx` | si | `boolean reaper.SetTrackMIDINoteNameEx(ReaProject proj, MediaTrack track, i` | channel < 0 assigns note name to all channels. pitch 128 assigns name for CC0, |

### Tempo, comps y curvas de tiempo

_Remaster rapido: llevar la cancion al tempo del proyecto._

| funcion | libre | firma | que hace |
|---|---|---|---|
| `AddTempoTimeSigMarker` | si | `boolean reaper.AddTempoTimeSigMarker(ReaProject proj, number timepos, numb` | Deprecated. Use SetTempoTimeSigMarker with ptidx=-1. |
| `EditTempoTimeSigMarker` | si | `boolean reaper.EditTempoTimeSigMarker(ReaProject project, integer markerin` | Open the tempo/time signature marker editor dialog. |
| `FindTempoTimeSigMarker` | si | `integer reaper.FindTempoTimeSigMarker(ReaProject project, number time)` | Find the tempo/time signature marker that falls at or before this time positio |
| `GetSetTempoTimeSigMarkerBasis` | si | `number reaper.GetSetTempoTimeSigMarkerBasis(ReaProject project, integer po` | Gets or sets the beat basis of a tempo/time signature marker. Supported values |
| `GetSetTempoTimeSigMarkerFlag` | si | `integer reaper.GetSetTempoTimeSigMarkerFlag(ReaProject project, integer po` | Gets or sets the attribute flag of a tempo/time signature marker. flag &1=sets |
| `Master_NormalizeTempo` | si | `number reaper.Master_NormalizeTempo(number bpm, boolean isnormalized)` | Convert the tempo to/from a value between 0 and 1, representing bpm in the ran |
| `MediaExplorerGetLastPlayedFileInfo` | si | `boolean retval, string filename, integer filemode, number selstart, number` | Get information about the most recently previewed Media Explorer file. filenam |
| `SetThemeColor` | si | `integer reaper.SetThemeColor(string ini_key, integer color, integer flags)` | Temporarily updates the theme color to the color specified (or the theme defau |
| `TimeMap2_GetNextChangeTime` | si | `number reaper.TimeMap2_GetNextChangeTime(ReaProject proj, number time)` | when does the next time map (tempo or time sig) change occur |
| `TimeMap_GetTimeSigAtTime` | si | `integer timesig_num, integer timesig_denom, number tempo = TimeMap_GetTime` | get the effective time signature and tempo |
| `TimeMap_QNToTime_abs` | si | `number reaper.TimeMap_QNToTime_abs(ReaProject proj, number qn)` | Converts project quarter note count (QN) to time. QN is counted from the start |
| `TimeMap_timeToQN_abs` | si | `number reaper.TimeMap_timeToQN_abs(ReaProject proj, number tpos)` | Converts project time position to quarter note count (QN). QN is counted from  |
| `TrackFX_GetNamedConfigParm` | si | `boolean retval, string buf = TrackFX_GetNamedConfigParm(MediaTrack track, ` | gets plug-in specific named configuration value (returns true on success). Sup |
| `CountTempoTimeSigMarkers` |  | `integer reaper.CountTempoTimeSigMarkers(ReaProject proj)` | Count the number of tempo/time signature markers in the project. See GetTempoT |

_... y 10 mas; `daw_api` las encuentra por palabra._

### Renderizar y exportar

_Entregar el remaster. Sin esto no hay final._

| funcion | libre | firma | que hace |
|---|---|---|---|
| `CalcMediaSrcLoudness` | si | `integer reaper.CalcMediaSrcLoudness(PCM_source mediasource)` | Calculates loudness statistics of media via dry run render. Statistics will be |
| `EnumRegionRenderMatrix` | si | `MediaTrack reaper.EnumRegionRenderMatrix(ReaProject proj, integer regionin` | Enumerate which tracks will be rendered within this region when using the regi |
| `SetRegionRenderMatrix` | si | `reaper.SetRegionRenderMatrix(ReaProject proj, integer regionindex, MediaTr` | Add (flag > 0) or remove (flag < 0) a track from this region when using the re |
| `GetSetProjectInfo` |  | `number reaper.GetSetProjectInfo(ReaProject project, string desc, number va` | Get or set project information. RENDER_SETTINGS : (&(1/2)==0)=master mix, &1=s |
| `GetSetProjectInfo_String` |  | `boolean retval, string valuestrNeedBig = GetSetProjectInfo_String(ReaProje` | Get or set project information. PROJECT_NAME : project file name (read-only, i |

### Undo y state chunks

_CTransactions y volcado de estado: el escape cuando no hay API directa._

| funcion | libre | firma | que hace |
|---|---|---|---|
| `CSurf_FlushUndo` | si | `reaper.CSurf_FlushUndo(boolean force)` | call this to force flushing of the undo states after using CSurf_On*Change() |
| `GetEnvelopeStateChunk` | si | `boolean retval, string str = GetEnvelopeStateChunk(TrackEnvelope env, stri` | Gets the RPPXML state of an envelope, returns true if successful. Undo flag is |
| `GetSetEnvelopeState` | si | `boolean retval, string str = GetSetEnvelopeState(TrackEnvelope env, string` | deprecated -- see SetEnvelopeStateChunk, GetEnvelopeStateChunk |
| `GetSetEnvelopeState2` | si | `boolean retval, string str = GetSetEnvelopeState2(TrackEnvelope env, strin` | deprecated -- see SetEnvelopeStateChunk, GetEnvelopeStateChunk |
| `GetSetItemState` | si | `boolean retval, string str = GetSetItemState(MediaItem item, string str)` | deprecated -- see SetItemStateChunk, GetItemStateChunk |
| `GetSetItemState2` | si | `boolean retval, string str = GetSetItemState2(MediaItem item, string str, ` | deprecated -- see SetItemStateChunk, GetItemStateChunk |
| `GetSetTrackState` | si | `boolean retval, string str = GetSetTrackState(MediaTrack track, string str` | deprecated -- see SetTrackStateChunk, GetTrackStateChunk |
| `GetSetTrackState2` | si | `boolean retval, string str = GetSetTrackState2(MediaTrack track, string st` | deprecated -- see SetTrackStateChunk, GetTrackStateChunk |
| `IsProjectDirty` | si | `integer reaper.IsProjectDirty(ReaProject proj)` | Is the project dirty (needing save)? Always returns 0 if 'undo/prompt to save' |
| `MarkProjectDirty` | si | `reaper.MarkProjectDirty(ReaProject proj)` | Marks project as dirty (needing save) if 'undo/prompt to save' is enabled in p |
| `SetEnvelopeStateChunk` | si | `boolean reaper.SetEnvelopeStateChunk(TrackEnvelope env, string str, boolea` | Sets the RPPXML state of an envelope, returns true if successful. Undo flag is |
| `Undo_GetCurEntry` | si | `integer reaper.Undo_GetCurEntry(ReaProject proj)` | Gets current undo entry index |
| `Undo_GetEntryDesc` | si | `string reaper.Undo_GetEntryDesc(ReaProject proj, integer index)` | Gets description of undo entry. |
| `Undo_GetEntryTime` | si | `number reaper.Undo_GetEntryTime(ReaProject proj, integer index)` | Gets timestamp (since Jan 1 1970) of undo entry. |

_... y 7 mas; `daw_api` las encuentra por palabra._

### MIDI en general

_Escribir y leer notas. 94 funciones, el bridge solo usa 25._

| funcion | libre | firma | que hace |
|---|---|---|---|
| `Audio_Init` | si | `reaper.Audio_Init()` | open all audio and MIDI devices, if not open |
| `Audio_Quit` | si | `reaper.Audio_Quit()` | close all audio and MIDI devices, if open |
| `EnumTrackMIDIProgramNamesEx` | si | `boolean retval, string programName = EnumTrackMIDIProgramNamesEx(ReaProjec` | returns false if there are no plugins on the track that support MIDI programs, |
| `GetInputActivityLevel` | si | `number reaper.GetInputActivityLevel(integer input_id)` | returns approximate input level if available, 0-511 mono inputs, /1024 for ste |
| `GetMaxMidiInputs` | si | `integer reaper.GetMaxMidiInputs()` | returns max dev for midi inputs/outputs |
| `GetMediaSourceSampleRate` | si | `integer reaper.GetMediaSourceSampleRate(PCM_source source)` | Returns the sample rate. MIDI source media will return zero. |
| `GetMediaSourceType` | si | `string typebuf = GetMediaSourceType(PCM_source source)` | copies the media source type ("WAV", "MIDI", etc) to typebuf |
| `GetNumMIDIInputs` | si | `integer reaper.GetNumMIDIInputs()` | returns max number of real midi hardware inputs |
| `GetNumMIDIOutputs` | si | `integer reaper.GetNumMIDIOutputs()` | returns max number of real midi hardware outputs |
| `GetToggleCommandStateEx` | si | `integer reaper.GetToggleCommandStateEx(integer section_id, integer command` | Returns the toggle state of the action. section 0=main, 100=main alt, 32060=MI |
| `GetTrackMIDILyrics` | si | `boolean retval, string buf = GetTrackMIDILyrics(MediaTrack track, integer ` | Get all MIDI lyrics on the track. Lyrics will be returned as one string with t |
| `HasTrackMIDIProgramsEx` | si | `string reaper.HasTrackMIDIProgramsEx(ReaProject proj, MediaTrack track)` | returns name of track plugin that is supplying MIDI programs,or NULL if there  |
| `MIDIEditorFlagsForTrack` | si | `integer pitchwheelrange, integer flags = MIDIEditorFlagsForTrack(MediaTrac` | Get or set MIDI editor settings for this track. pitchwheelrange: semitones up  |
| `MIDIEditor_EnumTakes` | si | `MediaItem_Take reaper.MIDIEditor_EnumTakes(HWND midieditor, integer takein` | list the takes that are currently being edited in this MIDI editor, starting w |

_... y 67 mas; `daw_api` las encuentra por palabra._

## 5. Los parametros con nombre (475 en 26 funciones)

Un `D_VOL` mal puesto no da error: cambia otra cosa en silencio. Estos son los que hay que mirar antes de escribir.

### `GetMediaTrackInfo_Value` â€” 69 claves

`number reaper.GetMediaTrackInfo_Value(MediaTrack tr, string parmname)`

| clave | que es | clave | que es |
|---|---|---|---|
| `B_AUTO_RECARM` | bool * : automatically set record arm when sel | `I_HEIGHTOVERRIDE` | int * : custom height override for TCP window, |
| `B_MAINSEND` | bool * : track sends audio to parent | `I_MCPH` | int * : current MCP height in pixels (read-onl |
| `B_MUTE` | bool * : muted | `I_MCPSCREENX` | int * : current MCP X-position in pixels relat |
| `B_PHASE` | bool * : track phase inverted | `I_MCPW` | int * : current MCP width in pixels (read-only |
| `B_RECMON_IN_EFFECT` | bool * : record monitoring in effect (current  | `I_MCPX` | int * : current MCP X-position in pixels relat |
| `B_SHOWINMIXER` | bool * : track control panel visible in mixer  | `I_MCPY` | int * : current MCP Y-position in pixels relat |
| `B_SHOWINTCP` | bool * : track control panel visible in arrang | `I_MIDIHWOUT` | int * : track midi hardware output index, <0=d |
| `B_SOLO_DEFEAT` | bool * : when set, if anything else is soloed  | `I_MIDIHWOUT_SLOT` | int * : hint for slot index for MIDI HW output |
| `B_TCPPIN` | bool * : track is pinned to top of arrange vie | `I_MIDI_CTL_CHAN` | int * : -1 no link, 0-15 link to MIDI volume/p |
| `C_ALLLANESPLAY` | char * : on fixed lane tracks, 0=no lanes play | `I_MIDI_INPUT_CHANMAP` | int * : -1 maps to source channel, otherwise 1 |
| `C_BEATATTACHMODE` | char * : track timebase, -1=project default, 0 | `I_MIDI_TRACKSEL_FLAG` | int * : MIDI editor track list options: &1=exp |
| `C_LANEPLAYS` | N : char * : on fixed lane tracks, 0=lane N do | `I_NCHAN` | int * : number of track channels, 2-128, even  |
| `C_LANESCOLLAPSED` | char * : fixed lane collapse state (1=lanes co | `I_NUMFIXEDLANES` | int * : number of track fixed lanes (fine to c |
| `C_LANESETTINGS` | char * : fixed lane settings (&1=auto-remove e | `I_PANLAW_FLAGS` | int * : pan law flags, 0=sine taper, 1=hybrid  |
| `C_MAINSEND_NCH` | char * : channel count of track send to parent | `I_PANMODE` | int * : pan mode, 0=classic 3.x, 3=new balance |
| `C_MAINSEND_OFFS` | char * : channel offset of track send to paren | `I_PERFFLAGS` | int * : track performance flags, &1=no media b |
| `D_DUALPANL` | double * : dualpan position 1, -1..1, only if  | `I_PLAY_OFFSET_FLAG` | int * : track media playback offset state, &1= |
| `D_DUALPANR` | double * : dualpan position 2, -1..1, only if  | `I_RECARM` | int * : record armed, 0=not record armed, 1=re |
| `D_PAN` | double * : trim pan of track, -1..1 | `I_RECINPUT` | int * : record input, <0=no input. if 4096 set |
| `D_PANLAW` | double * : pan law of track, <0=project defaul | `I_RECMODE` | int * : record mode, 0=input, 1=stereo out, 2= |
| `D_PLAY_OFFSET` | double * : track media playback offset, units  | `I_RECMODE_FLAGS` | int * : record mode flags, &3=output recording |
| `D_VOL` | double * : trim volume of track, 0=-inf, 0.5=- | `I_RECMON` | int * : record monitoring, 0=off, 1=normal, 2= |
| `D_WIDTH` | double * : width of track, -1..1 | `I_RECMONITEMS` | int * : monitor items while recording, 0=off,  |
| `F_MCP_FXPARM_SCALE` | float * : scale of fx parameter area in MCP (0 | `I_SELECTED` | int * : track selected, 0=unselected, 1=select |
| `F_MCP_FXSEND_SCALE` | float * : scale of fx+send area in MCP (0=mini | `I_SOLO` | int * : soloed, 0=not soloed, 1=soloed, 2=solo |
| `F_MCP_SENDRGN_SCALE` | float * : scale of send area as proportion of  | `I_SPACER` | int * : 1=TCP track spacer above this trackB_H |
| `F_TCP_FXPARM_SCALE` | float * : scale of TCP parameter area when TCP | `I_TCPH` | int * : current TCP height in pixels not inclu |
| `IP_TRACKNUMBER` | int : track number 1-based, 0=not found, -1=ma | `I_TCPSCREENY` | int * : current TCP Y-position in pixels relat |
| `I_AUTOMODE` | int * : track automation mode, 0=trim/off, 1=r | `I_TCPY` | int * : current TCP Y-position in pixels relat |
| `I_CUSTOMCOLOR` | int * : custom color, OS dependent color/0x100 | `I_VUMODE` | int * : track vu mode, &1:disabled, &30==0:ste |
| `I_FOLDERCOMPACT` | int * : folder collapsed state (only valid on  | `I_WNDH` | int * : current TCP height in pixels including |
| `I_FOLDERDEPTH` | int * : folder depth change, 0=normal, 1=track | `P_ENV` | {GUID... : TrackEnvelope * : (read-only) chunk |
| `I_FREEMODE` | int * : 1=track free item positioning enabled, | `P_PARTRACK` | MediaTrack * : parent track (read-only) |
| `I_FREEZECOUNT` | int * : (read-only) freeze state count | `P_PROJECT` | ReaProject * : parent project (read-only) |
| `I_FXEN` | int * : fx enabled, 0=bypassed, !0=fx active | `` |  |

### `SetMediaTrackInfo_Value` â€” 67 claves

`boolean reaper.SetMediaTrackInfo_Value(MediaTrack tr, string parmname, number newvalue)`

| clave | que es | clave | que es |
|---|---|---|---|
| `B_AUTO_RECARM` | bool * : automatically set record arm when sel | `I_FXEN` | int * : fx enabled, 0=bypassed, !0=fx active |
| `B_MAINSEND` | bool * : track sends audio to parent | `I_HEIGHTOVERRIDE` | int * : custom height override for TCP window, |
| `B_MUTE` | bool * : muted | `I_MCPH` | int * : current MCP height in pixels (read-onl |
| `B_PHASE` | bool * : track phase inverted | `I_MCPSCREENX` | int * : current MCP X-position in pixels relat |
| `B_RECMON_IN_EFFECT` | bool * : record monitoring in effect (current  | `I_MCPW` | int * : current MCP width in pixels (read-only |
| `B_SHOWINMIXER` | bool * : track control panel visible in mixer  | `I_MCPX` | int * : current MCP X-position in pixels relat |
| `B_SHOWINTCP` | bool * : track control panel visible in arrang | `I_MCPY` | int * : current MCP Y-position in pixels relat |
| `B_SOLO_DEFEAT` | bool * : when set, if anything else is soloed  | `I_MIDIHWOUT` | int * : track midi hardware output index, <0=d |
| `B_TCPPIN` | bool * : track is pinned to top of arrange vie | `I_MIDIHWOUT_SLOT` | int * : hint for slot index for MIDI HW output |
| `C_ALLLANESPLAY` | char * : on fixed lane tracks, 0=no lanes play | `I_MIDI_CTL_CHAN` | int * : -1 no link, 0-15 link to MIDI volume/p |
| `C_BEATATTACHMODE` | char * : track timebase, -1=project default, 0 | `I_MIDI_INPUT_CHANMAP` | int * : -1 maps to source channel, otherwise 1 |
| `C_LANEPLAYS` | N : char * : on fixed lane tracks, 0=lane N do | `I_MIDI_TRACKSEL_FLAG` | int * : MIDI editor track list options: &1=exp |
| `C_LANESCOLLAPSED` | char * : fixed lane collapse state (1=lanes co | `I_NCHAN` | int * : number of track channels, 2-128, even  |
| `C_LANESETTINGS` | char * : fixed lane settings (&1=auto-remove e | `I_NUMFIXEDLANES` | int * : number of track fixed lanes (fine to c |
| `C_MAINSEND_NCH` | char * : channel count of track send to parent | `I_PANLAW_FLAGS` | int * : pan law flags, 0=sine taper, 1=hybrid  |
| `C_MAINSEND_OFFS` | char * : channel offset of track send to paren | `I_PANMODE` | int * : pan mode, 0=classic 3.x, 3=new balance |
| `D_DUALPANL` | double * : dualpan position 1, -1..1, only if  | `I_PERFFLAGS` | int * : track performance flags, &1=no media b |
| `D_DUALPANR` | double * : dualpan position 2, -1..1, only if  | `I_PLAY_OFFSET_FLAG` | int * : track media playback offset state, &1= |
| `D_PAN` | double * : trim pan of track, -1..1 | `I_RECARM` | int * : record armed, 0=not record armed, 1=re |
| `D_PANLAW` | double * : pan law of track, <0=project defaul | `I_RECINPUT` | int * : record input, <0=no input. if 4096 set |
| `D_PLAY_OFFSET` | double * : track media playback offset, units  | `I_RECMODE` | int * : record mode, 0=input, 1=stereo out, 2= |
| `D_VOL` | double * : trim volume of track, 0=-inf, 0.5=- | `I_RECMODE_FLAGS` | int * : record mode flags, &3=output recording |
| `D_WIDTH` | double * : width of track, -1..1 | `I_RECMON` | int * : record monitoring, 0=off, 1=normal, 2= |
| `F_MCP_FXPARM_SCALE` | float * : scale of fx parameter area in MCP (0 | `I_RECMONITEMS` | int * : monitor items while recording, 0=off,  |
| `F_MCP_FXSEND_SCALE` | float * : scale of fx+send area in MCP (0=mini | `I_SELECTED` | int * : track selected, 0=unselected, 1=select |
| `F_MCP_SENDRGN_SCALE` | float * : scale of send area as proportion of  | `I_SOLO` | int * : soloed, 0=not soloed, 1=soloed, 2=solo |
| `F_TCP_FXPARM_SCALE` | float * : scale of TCP parameter area when TCP | `I_SPACER` | int * : 1=TCP track spacer above this trackB_H |
| `IP_TRACKNUMBER` | int : track number 1-based, 0=not found, -1=ma | `I_TCPH` | int * : current TCP height in pixels not inclu |
| `I_AUTOMODE` | int * : track automation mode, 0=trim/off, 1=r | `I_TCPSCREENY` | int * : current TCP Y-position in pixels relat |
| `I_CUSTOMCOLOR` | int * : custom color, OS dependent color/0x100 | `I_TCPY` | int * : current TCP Y-position in pixels relat |
| `I_FOLDERCOMPACT` | int * : folder collapsed state (only valid on  | `I_VUMODE` | int * : track vu mode, &1:disabled, &30==0:ste |
| `I_FOLDERDEPTH` | int * : folder depth change, 0=normal, 1=track | `I_WNDH` | int * : current TCP height in pixels including |
| `I_FREEMODE` | int * : 1=track free item positioning enabled, | `P_ENV` | {GUID... : TrackEnvelope * : (read-only) chunk |
| `I_FREEZECOUNT` | int * : (read-only) freeze state count | `` |  |

### `GetMediaItemTakeInfo_Value` â€” 54 claves

`number reaper.GetMediaItemTakeInfo_Value(MediaItem_Take take, string parmname)`

| clave | que es | clave | que es |
|---|---|---|---|
| `ADD` | int : read or write this key to add a new spec | `F_STRETCHFADESIZE` | float * : stretch marker fade size in seconds  |
| `BOTFREQ_ADD` | pos:val : int * : reading or writing will inse | `GAIN` | float * : gain of spectral edit |
| `BOTFREQ_CNT` | int * : number of bottom frequency-points | `GATE_FLOOR` | float * : gate floor |
| `BOTFREQ_DEL` | y : int * : reading or writing will delete bot | `GATE_THRESH` | float * : gate threshold |
| `BOTFREQ_FREQ` | y : float * : (read-only) get frequency of bot | `IP_SPECEDIT` |  |
| `BOTFREQ_POS` | y : float * : (read-only) get position of bott | `IP_TAKENUMBER` | int : take number (read-only, returns the take |
| `B_PPITCH` | bool * : preserve pitch when changing playback | `I_CHANMODE` | int * : channel mode, 0=normal, 1=reverse ster |
| `B_SPECEDIT` | x: | `I_CUSTOMCOLOR` | int * : custom color, OS dependent color/0x100 |
| `CHAN` | int * : channel index, -1 for omni | `I_LASTH` | int * : height in pixels (read-only) |
| `CNT` | int : spectral edit count (read-only) | `I_LASTY` | int * : Y-position (relative to top of track)  |
| `COMP_RATIO` | float * : comp ratio | `I_PITCHMODE` | int * : pitch shifter mode, -1=project default |
| `COMP_THRESH` | float * : comp threshold | `I_RECPASSID` | int * : record pass ID |
| `DELETE` | x : int : read or write this key to remove the | `I_SPECEDIT` | x: |
| `D_PAN` | double * : take pan, -1..1 | `I_STRETCHFLAGS` | int * : stretch marker flags (&7 mask for mode |
| `D_PANLAW` | double * : take pan law, -1=default, 0.5=-6dB, | `I_TAKEFX_NCH` | int * : number of internal audio channels for  |
| `D_PITCH` | double * : take pitch adjustment in semitones, | `LENGTH` | double * : length of spectral edit |
| `D_PLAYRATE` | double * : take playback rate, 0.5=half speed, | `POSITION` | double * : position of spectral edit start (ch |
| `D_SPECEDIT` | x: | `P_ITEM` | pointer to MediaItem (read-only) |
| `D_STARTOFFS` | double * : start offset in source media, in se | `P_SOURCE` | PCM_source *. Note that if setting this, you s |
| `D_VOL` | double * : take volume, 0=-inf, 0.5=-6dB, 1=+0 | `P_TRACK` | pointer to MediaTrack (read-only) |
| `FADE_HI` | float * : fade-hf size 0..1 | `SELECTED` | bool * : selection state |
| `FADE_IN` | float * : fade-in size 0..1 | `SORT` | int : read or write this key to re-sort spectr |
| `FADE_LOW` | float * : fade-lf size 0..1 | `TOPFREQ_ADD` | pos:val : int * : reading or writing will inse |
| `FADE_OUT` | float * : fade-out size 0..1 | `TOPFREQ_CNT` | int * : (read-only) number of top frequency-po |
| `FFT_SIZE` | int * : FFT size used by spectral edits for th | `TOPFREQ_DEL` | y : int * : reading or writing will delete top |
| `FLAGS` | int * : flags, &1=bypassed, &2=solo | `TOPFREQ_FREQ` | y : float * : (read-only) get frequency of top |
| `F_SPECEDIT` | x: | `TOPFREQ_POS` | y : float * : (read-only) get position of top  |

### `SetMediaItemTakeInfo_Value` â€” 51 claves

`boolean reaper.SetMediaItemTakeInfo_Value(MediaItem_Take take, string parmname, number newvalue)`

| clave | que es | clave | que es |
|---|---|---|---|
| `ADD` | int : read or write this key to add a new spec | `F_SPECEDIT` | x: |
| `BOTFREQ_ADD` | pos:val : int * : reading or writing will inse | `F_STRETCHFADESIZE` | float * : stretch marker fade size in seconds  |
| `BOTFREQ_CNT` | int * : number of bottom frequency-points | `GAIN` | float * : gain of spectral edit |
| `BOTFREQ_DEL` | y : int * : reading or writing will delete bot | `GATE_FLOOR` | float * : gate floor |
| `BOTFREQ_FREQ` | y : float * : (read-only) get frequency of bot | `GATE_THRESH` | float * : gate threshold |
| `BOTFREQ_POS` | y : float * : (read-only) get position of bott | `IP_SPECEDIT` |  |
| `B_PPITCH` | bool * : preserve pitch when changing playback | `IP_TAKENUMBER` | int : take number (read-only, returns the take |
| `B_SPECEDIT` | x: | `I_CHANMODE` | int * : channel mode, 0=normal, 1=reverse ster |
| `CHAN` | int * : channel index, -1 for omni | `I_CUSTOMCOLOR` | int * : custom color, OS dependent color/0x100 |
| `CNT` | int : spectral edit count (read-only) | `I_LASTH` | int * : height in pixels (read-only) |
| `COMP_RATIO` | float * : comp ratio | `I_LASTY` | int * : Y-position (relative to top of track)  |
| `COMP_THRESH` | float * : comp threshold | `I_PITCHMODE` | int * : pitch shifter mode, -1=project default |
| `DELETE` | x : int : read or write this key to remove the | `I_RECPASSID` | int * : record pass ID |
| `D_PAN` | double * : take pan, -1..1 | `I_SPECEDIT` | x: |
| `D_PANLAW` | double * : take pan law, -1=default, 0.5=-6dB, | `I_STRETCHFLAGS` | int * : stretch marker flags (&7 mask for mode |
| `D_PITCH` | double * : take pitch adjustment in semitones, | `I_TAKEFX_NCH` | int * : number of internal audio channels for  |
| `D_PLAYRATE` | double * : take playback rate, 0.5=half speed, | `LENGTH` | double * : length of spectral edit |
| `D_SPECEDIT` | x: | `POSITION` | double * : position of spectral edit start (ch |
| `D_STARTOFFS` | double * : start offset in source media, in se | `SELECTED` | bool * : selection state |
| `D_VOL` | double * : take volume, 0=-inf, 0.5=-6dB, 1=+0 | `SORT` | int : read or write this key to re-sort spectr |
| `FADE_HI` | float * : fade-hf size 0..1 | `TOPFREQ_ADD` | pos:val : int * : reading or writing will inse |
| `FADE_IN` | float * : fade-in size 0..1 | `TOPFREQ_CNT` | int * : (read-only) number of top frequency-po |
| `FADE_LOW` | float * : fade-lf size 0..1 | `TOPFREQ_DEL` | y : int * : reading or writing will delete top |
| `FADE_OUT` | float * : fade-out size 0..1 | `TOPFREQ_FREQ` | y : float * : (read-only) get frequency of top |
| `FFT_SIZE` | int * : FFT size used by spectral edits for th | `TOPFREQ_POS` | y : float * : (read-only) get position of top  |
| `FLAGS` | int * : flags, &1=bypassed, &2=solo | `` |  |

### `GetSetProjectInfo` â€” 46 claves

`number reaper.GetSetProjectInfo(ReaProject project, string desc, number value, boolean is_set)`

| clave | que es | clave | que es |
|---|---|---|---|
| `ARRANGE_H` | arrange view height in pixels (read-only) | `RENDER_NORMALIZE_TARGET` | render normalization target (0.5 means -6.02dB |
| `ARRANGE_MIN_TIMESCALE` | arrange view minimum time scale (horizontal zo | `RENDER_PADEND` | pad render end with silence (0.001 means 1ms,  |
| `ARRANGE_W` | arrange view width in pixels (read-only) | `RENDER_PADSTART` | pad render start with silence (0.001 means 1ms |
| `DIRTY` | set to 1 if project was modified since last sa | `RENDER_SETTINGS` | (&(1/2)==0)=master mix, &1=stems+master mix, & |
| `PROJECT_SRATE` | sample rate (ignored unless PROJECT_SRATE_USE  | `RENDER_SRATE` | sample rate of rendered file (or 0 for project |
| `PROJECT_SRATE_USE` | set to 1 if project sample rate is used | `RENDER_STARTPOS` | render start time when RENDER_BOUNDSFLAG=0 |
| `PROJECT_TCP_UI_FLAGS` | &1=pinning tracks to top of arrange view is ov | `RENDER_TAILFLAG` | apply render tail setting when rendering: &1=c |
| `PROJECT_TIMEBASE` | 0=time, 1=beats position, length, rate, 2=beat | `RENDER_TAILMS` | tail length in ms to render (only used if REND |
| `PROJECT_TIMEBASE_FLAGS` | &1=timebase affects MIDI items, &2=in beats ti | `RENDER_TRIMEND` | trim render end threshold (0.5 means -6.02dB,  |
| `READONLY` | set to 1 if project is opened read-only, 0 if  | `RENDER_TRIMSTART` | trim render start threshold (0.5 means -6.02dB |
| `RENDER_ADDTOPROJ` | &1=add rendered files to project, &2=do not re | `RULER_HEIGHT` | ruler height in pixels |
| `RENDER_BOUNDSFLAG` | 0=custom time bounds, 1=entire project, 2=time | `RULER_LANE_COLOR` | X : ruler lane default color, color&0x1000000  |
| `RENDER_BRICKWALL` | render brickwall limit (0.5 means -6.02dB, req | `RULER_LANE_COUNT` | number of ruler lanes |
| `RENDER_CHANNELS` | number of channels in rendered file | `RULER_LANE_DEFAULT` | X : 1 if ruler lane is default for new regions |
| `RENDER_DELAY` | seconds to delay start of render to allow FX t | `RULER_LANE_FROM_GUID` | X : ruler lane number with unique identifier X |
| `RENDER_DITHER` | &1=dither, &2=noise shaping, &4=dither stems,  | `RULER_LANE_HIDDEN` | X : 1 if ruler lane is hidden, 0 otherwise |
| `RENDER_ENDPOS` | render end time when RENDER_BOUNDSFLAG=0 | `RULER_LANE_LOCKED` | X : 1 if ruler lane is locked, 0 otherwise |
| `RENDER_FADEIN` | render fade-in (0.001 means 1 ms, requires REN | `RULER_LANE_ORDER` | X : move lane at position X to a new position, |
| `RENDER_FADEINSHAPE` | render fade-in shape | `RULER_LANE_TIMEBASE` | X : ruler lane default timebase, -1=project de |
| `RENDER_FADELPF` | render low pass frequency fade, &1=fade-in, &2 | `RULER_LANE_VISIBLE` | X : 1 if ruler lane is visible (not hidden and |
| `RENDER_FADEOUT` | render fade-out (0.001 means 1 ms, requires RE | `VKB_CHANNEL` | virtual MIDI keyboard channel |
| `RENDER_FADEOUTSHAPE` | render fade-out shape | `VKB_LASTVEL` | virtual MIDI keyboard last velocity |
| `RENDER_NORMALIZE` | &1=enable normalization, (&14==0)=LUFS-I, (&14 | `VKB_NOTECENTER` | virtual MIDI keyboard center note |

### `GetMediaItemInfo_Value` â€” 34 claves

`number reaper.GetMediaItemInfo_Value(MediaItem item, string parmname)`

| clave | que es | clave | que es |
|---|---|---|---|
| `B_ALLTAKESPLAY` | bool * : all takes play | `D_FADEOUTLEN` | double * : item manual fadeout length in secon |
| `B_FIXEDLANE_HIDDEN` | bool * : true if displaying only one fixed lan | `D_FADEOUTLEN_AUTO` | double * : item auto-fadeout length in seconds |
| `B_LOOPSRC` | bool * : loop source | `D_LENGTH` | double * : item length in seconds |
| `B_MUTE` | bool * : muted (item solo overrides). setting  | `D_POSITION` | double * : item position in seconds |
| `B_MUTE_ACTUAL` | bool * : muted (ignores solo). setting this va | `D_SNAPOFFSET` | double * : item snap offset in seconds |
| `B_UISEL` | bool * : selected in arrange view | `D_VOL` | double * : item volume, 0=-inf, 0.5=-6dB, 1=+0 |
| `C_AUTOSTRETCH` | : char * : auto-stretch at project tempo chang | `F_FREEMODE_H` | float * : free item positioning or fixed lane  |
| `C_BEATATTACHMODE` | char * : item timebase, -1=track or project de | `F_FREEMODE_Y` | float * : free item positioning or fixed lane  |
| `C_FADEINSHAPE` | int * : fadein shape, 0..6, 0=linear | `IP_ITEMNUMBER` | int : item number on this track (read-only, re |
| `C_FADEOUTSHAPE` | int * : fadeout shape, 0..6, 0=linear | `I_CURTAKE` | int * : active take number |
| `C_LANEPLAYS` | char * : on fixed lane tracks, 0=this item lan | `I_CUSTOMCOLOR` | int * : custom color, OS dependent color/0x100 |
| `C_LOCK` | char * : locked, &1=locked | `I_FADELPF` | int * : low pass frequency fade, &1=fade-in, & |
| `C_MUTE_SOLO` | char * : solo override (-1=soloed, 0=no overri | `I_FIXEDLANE` | int * : fixed lane of item (fine to call with  |
| `D_FADEINDIR` | double * : item fadein curvature, -1..1 | `I_GROUPID` | int * : group ID, 0=no group |
| `D_FADEINLEN` | double * : item manual fadein length in second | `I_LASTH` | int * : height in pixels (read-only) |
| `D_FADEINLEN_AUTO` | double * : item auto-fadein length in seconds, | `I_LASTY` | int * : Y-position (relative to top of track)  |
| `D_FADEOUTDIR` | double * : item fadeout curvature, -1..1 | `P_TRACK` | MediaTrack * : (read-only) |

### `SetMediaItemInfo_Value` â€” 33 claves

`boolean reaper.SetMediaItemInfo_Value(MediaItem item, string parmname, number newvalue)`

| clave | que es | clave | que es |
|---|---|---|---|
| `B_ALLTAKESPLAY` | bool * : all takes play | `D_FADEOUTLEN` | double * : item manual fadeout length in secon |
| `B_FIXEDLANE_HIDDEN` | bool * : true if displaying only one fixed lan | `D_FADEOUTLEN_AUTO` | double * : item auto-fadeout length in seconds |
| `B_LOOPSRC` | bool * : loop source | `D_LENGTH` | double * : item length in seconds |
| `B_MUTE` | bool * : muted (item solo overrides). setting  | `D_POSITION` | double * : item position in seconds |
| `B_MUTE_ACTUAL` | bool * : muted (ignores solo). setting this va | `D_SNAPOFFSET` | double * : item snap offset in seconds |
| `B_UISEL` | bool * : selected in arrange view | `D_VOL` | double * : item volume, 0=-inf, 0.5=-6dB, 1=+0 |
| `C_AUTOSTRETCH` | : char * : auto-stretch at project tempo chang | `F_FREEMODE_H` | float * : free item positioning or fixed lane  |
| `C_BEATATTACHMODE` | char * : item timebase, -1=track or project de | `F_FREEMODE_Y` | float * : free item positioning or fixed lane  |
| `C_FADEINSHAPE` | int * : fadein shape, 0..6, 0=linear | `IP_ITEMNUMBER` | int : item number on this track (read-only, re |
| `C_FADEOUTSHAPE` | int * : fadeout shape, 0..6, 0=linear | `I_CURTAKE` | int * : active take number |
| `C_LANEPLAYS` | char * : on fixed lane tracks, 0=this item lan | `I_CUSTOMCOLOR` | int * : custom color, OS dependent color/0x100 |
| `C_LOCK` | char * : locked, &1=locked | `I_FADELPF` | int * : low pass frequency fade, &1=fade-in, & |
| `C_MUTE_SOLO` | char * : solo override (-1=soloed, 0=no overri | `I_FIXEDLANE` | int * : fixed lane of item (fine to call with  |
| `D_FADEINDIR` | double * : item fadein curvature, -1..1 | `I_GROUPID` | int * : group ID, 0=no group |
| `D_FADEINLEN` | double * : item manual fadein length in second | `I_LASTH` | int * : height in pixels (read-only) |
| `D_FADEINLEN_AUTO` | double * : item auto-fadein length in seconds, | `I_LASTY` | int * : Y-position (relative to top of track)  |
| `D_FADEOUTDIR` | double * : item fadeout curvature, -1..1 | `` |  |

### `GetSetProjectInfo_String` â€” 24 claves

`boolean retval, string valuestrNeedBig = reaper.GetSetProjectInfo_String(ReaProject project, string desc, string valuestrNeedBig, boolean is_set)`

| clave | que es | clave | que es |
|---|---|---|---|
| `APPLYFX_FORMAT` | base64-encoded sink configuration (see project | `RENDER_EXTRAFILEDIR` | alternate path for renderedfile.wav.rpp and re |
| `ID3` | TALB/my album name" to set. Call with valuestr | `RENDER_FILE` | render directory |
| `MARKER_GUID` | X : discouraged. see GetRegionOrMarker, GetSet | `RENDER_FORMAT` | base64-encoded sink configuration (see project |
| `MARKER_INDEX_FROM_GUID` | {GUID} : discouraged. see GetRegionOrMarker, G | `RENDER_FORMAT2` | base64-encoded secondary sink configuration. C |
| `OPENCOPY_CFGIDX` | integer for the configuration of format to use | `RENDER_METADATA` | get or set the metadata saved with the project |
| `PROJECT_AUTHOR` | author field from Project Settings/Notes dialo | `RENDER_PATTERN` | render file name (may contain wildcards) |
| `PROJECT_NAME` | project file name (read-only, is_set will be i | `RENDER_STATS` | (read-only) semicolon separated list of statis |
| `PROJECT_TITLE` | title field from Project Settings/Notes dialog | `RENDER_STATS_SUMMARY` | (read-only) human-readable summary of statisti |
| `RECORD_FORMAT` | base64-encoded sink configuration (see project | `RENDER_TARGETS` | semicolon separated list of files that would b |
| `RECORD_PATH` | recording directory -- may be blank or a relat | `RULER_LANE_GUID` | X : ruler lane unique identifier |
| `RECORD_PATH_SECONDARY` | secondary recording directory | `RULER_LANE_NAME` | X : ruler lane name |
| `RECTAG` | project recording tag wildcard ($rectag). Can  | `TRACK_GROUP_NAME` | X : track group name, X should be 1..64 |

### `GetTrackSendInfo_Value` â€” 15 claves

`number reaper.GetTrackSendInfo_Value(MediaTrack tr, integer category, integer sendidx, string parmname)`

| clave | que es | clave | que es |
|---|---|---|---|
| `B_MONO` | bool * | `I_MIDIFLAGS` | int * : low 5 bits=source channel 0=all, 1-16, |
| `B_MUTE` | bool * | `I_SENDMODE` | int * : 0=post-fader, 1=pre-fx, 2=post-fx (dep |
| `B_PHASE` | bool * : true to flip phase | `I_SLOT_HINT` | int * : hint for slot index in UI, or -1 for u |
| `D_PAN` | double * : -1..+1 | `I_SRCCHAN` | int * : -1 for no audio send. Low 10 bits spec |
| `D_PANLAW` | double * : 1.0=+0.0db, 0.5=-6dB, -1.0 = projde | `P_DESTTRACK` | MediaTrack * : destination track, only applies |
| `D_VOL` | double * : 1.0 = +0dB etc | `P_ENV` | <envchunkname : TrackEnvelope * : call with :< |
| `I_AUTOMODE` | int * : automation mode (-1=use track automode | `P_SRCTRACK` | MediaTrack * : source track, only applies for  |
| `I_DSTCHAN` | int * : low 10 bits are destination index, &10 | `` |  |

### `GetEnvelopeInfo_Value` â€” 12 claves

`number reaper.GetEnvelopeInfo_Value(TrackEnvelope env, string parmname)`

| clave | que es | clave | que es |
|---|---|---|---|
| `I_DISPLAYEDCOLOR` | int : displayed envelope color | `I_TCPY` | int : Y offset of envelope relative to parent  |
| `I_HWOUT_IDX` | int : 1-based index of hardware output in P_TR | `I_TCPY_USED` | int : Y offset of envelope relative to parent  |
| `I_RECV_IDX` | int : 1-based index of receive in P_DESTTRACK  | `P_DESTTRACK` | MediaTrack * : destination track pointer, if o |
| `I_SEND_IDX` | int : 1-based index of send in P_TRACK, or 0 i | `P_ITEM` | MediaItem * : parent item pointer (if any) |
| `I_TCPH` | int : visible height of envelope | `P_TAKE` | MediaItem_Take * : parent take pointer (if any |
| `I_TCPH_USED` | int : visible height of envelope, exclusive of | `P_TRACK` | MediaTrack * : parent track pointer (if any) |

### `SetTrackSendInfo_Value` â€” 12 claves

`boolean reaper.SetTrackSendInfo_Value(MediaTrack tr, integer category, integer sendidx, string parmname, number newvalue)`

| clave | que es | clave | que es |
|---|---|---|---|
| `B_MONO` | bool * | `I_AUTOMODE` | int * : automation mode (-1=use track automode |
| `B_MUTE` | bool * | `I_DSTCHAN` | int * : low 10 bits are destination index, &10 |
| `B_PHASE` | bool * : true to flip phase | `I_MIDIFLAGS` | int * : low 5 bits=source channel 0=all, 1-16, |
| `D_PAN` | double * : -1..+1 | `I_SENDMODE` | int * : 0=post-fader, 1=pre-fx, 2=post-fx (dep |
| `D_PANLAW` | double * : 1.0=+0.0db, 0.5=-6dB, -1.0 = projde | `I_SLOT_HINT` | int * : hint for slot index in UI, or -1 for u |
| `D_VOL` | double * : 1.0 = +0dB etc | `I_SRCCHAN` | int * : -1 for no audio send. Low 10 bits spec |

### `GetSetAutomationItemInfo` â€” 11 claves

`number reaper.GetSetAutomationItemInfo(TrackEnvelope env, integer autoitem_idx, string desc, number value, boolean is_set)`

| clave | que es | clave | que es |
|---|---|---|---|
| `D_AMPLITUDE` | double * : automation item amplitude in the ra | `D_POOL_ID` | double * : automation item pool ID (as an inte |
| `D_BASELINE` | double * : automation item baseline value in t | `D_POOL_QNLEN` | double * : automation item pooled source lengt |
| `D_LENGTH` | double * : automation item length in seconds | `D_POSITION` | double * : automation item timeline position i |
| `D_LOOPSRC` | double * : nonzero if the automation item cont | `D_STARTOFFS` | double * : automation item start offset in sec |
| `D_MUTE` | double * : nonzero if the automation item is m | `D_UISEL` | double * : nonzero if the automation item is s |
| `D_PLAYRATE` | double * : automation item playback rate | `` |  |

### `GetSetMediaTrackInfo_String` â€” 10 claves

`boolean retval, string stringNeedBig = reaper.GetSetMediaTrackInfo_String(MediaTrack tr, string parmname, string stringNeedBig, boolean setNewValue)`

| clave | que es | clave | que es |
|---|---|---|---|
| `GUID` | GUID * : 16-byte GUID, can query or update. If | `P_NAME` | char * : track name (on master returns NULL) |
| `P_EXT` | xyz : char * : extension-specific persistent d | `P_RAZOREDITS` | const char * : list of razor edit areas, as sp |
| `P_ICON` | const char * : track icon (full filename, or r | `P_RAZOREDITS_EXT` | const char * : list of razor edit areas, as co |
| `P_LANENAME` | n : char * : lane name (returns NULL for non-f | `P_TCP_LAYOUT` | const char * : layout name |
| `P_MCP_LAYOUT` | const char * : layout name | `P_UI_RECT` | tcp.mute : char * : read-only, allows querying |

### `TrackFX_AddByName` â€” 7 claves

`integer reaper.TrackFX_AddByName(MediaTrack track, string fxname, boolean recFX, integer instantiate)`

| clave | que es | clave | que es |
|---|---|---|---|
| `AU` |  | `VST` |  |
| `DX` | or | `VST2` |  |
| `FXADD` | 2e to only succeed if exactly 2 FX are selecte | `VST3` |  |
| `JS` | or | `` |  |

### `GetSetEnvelopeInfo_String` â€” 6 claves

`boolean retval, string stringNeedBig = reaper.GetSetEnvelopeInfo_String(TrackEnvelope env, string parmname, string stringNeedBig, boolean setNewValue)`

| clave | que es | clave | que es |
|---|---|---|---|
| `ACTIVE` | active state (bool as a string "0" or "1") | `P_EXT` | xyz : extension-specific persistent data Note  |
| `ARM` | armed state (bool...) | `SHOWLANE` | show envelope in separate lane (bool...) |
| `GUID` | (read-only) GUID as a string {xyz-....} | `VISIBLE` | visible state (bool...) |

### `TrackFX_GetNamedConfigParm` â€” 5 claves

`boolean retval, string buf = reaper.TrackFX_GetNamedConfigParm(MediaTrack track, integer fx, string parmname)`

| clave | que es | clave | que es |
|---|---|---|---|
| `PARMIDX` | read from this value to get container paramete | `TRUEPEAK` | [ReaLimit] NUMCHANNELS, NUMSPEAKERS |
| `RESETCHANNELS` | [ReaSurroundPan] ITEMx : [ReaVerb] state confi | `VIDEO_CODE` | [video processor] code force_auto_bypass : 0 o |
| `RSMODE` | [RS5k] general mode, resample mode | `` |  |

### `GetSetMediaItemTakeInfo_String` â€” 4 claves

`boolean retval, string stringNeedBig = reaper.GetSetMediaItemTakeInfo_String(MediaItem_Take tk, string parmname, string stringNeedBig, boolean setNewV`

| clave | que es | clave | que es |
|---|---|---|---|
| `GUID` | GUID * : 16-byte GUID, can query or update. If | `P_EXT` |  |
| `ORIGINAL_FILENAME` | char * : if media was copied on import, this w | `P_NAME` | char * : take name |

### `TrackFX_SetNamedConfigParm` â€” 4 claves

`boolean reaper.TrackFX_SetNamedConfigParm(MediaTrack track, integer fx, string parmname, string value)`

| clave | que es | clave | que es |
|---|---|---|---|
| `RESETCHANNELS` | [ReaSurroundPan] ITEMx : [ReaVerb] state confi | `TRUEPEAK` | [ReaLimit] NUMCHANNELS, NUMSPEAKERS |
| `RSMODE` | [RS5k] general mode, resample mode | `VIDEO_CODE` | [video processor] code force_auto_bypass : 0 o |

### `GetSetMediaItemInfo_String` â€” 3 claves

`boolean retval, string stringNeedBig = reaper.GetSetMediaItemInfo_String(MediaItem item, string parmname, string stringNeedBig, boolean setNewValue)`

| clave | que es | clave | que es |
|---|---|---|---|
| `GUID` | GUID * : 16-byte GUID, can query or update. If | `P_NOTES` | char * : item note text (do not write to retur |
| `P_EXT` | xyz : char * : extension-specific persistent d | `` |  |

### `GetSetAutomationItemInfo_String` â€” 2 claves

`boolean retval, string valuestrNeedBig = reaper.GetSetAutomationItemInfo_String(TrackEnvelope env, integer autoitem_idx, string desc, string valuestrN`

| clave | que es | clave | que es |
|---|---|---|---|
| `P_POOL_EXT` | xyz : char * : extension-specific persistent d | `P_POOL_NAME` | char * : name of the underlying automation ite |

### `GetSetTrackSendInfo_String` â€” 1 claves

`boolean retval, string stringNeedBig = reaper.GetSetTrackSendInfo_String(MediaTrack tr, integer category, integer sendidx, string parmname, string str`

| clave | que es | clave | que es |
|---|---|---|---|
| `P_EXT` | xyz : char * : extension-specific persistent d | `` |  |

### `MB` â€” 1 claves

`integer reaper.MB(string msg, string title, integer type)`

| clave | que es | clave | que es |
|---|---|---|---|
| `RETRYCANCEL` | ret 1=OK,2=CANCEL,3=ABORT,4=RETRY,5=IGNORE,6=Y | `` |  |

### `SetThemeColor` â€” 1 claves

`integer reaper.SetThemeColor(string ini_key, integer color, integer flags)`

| clave | que es | clave | que es |
|---|---|---|---|
| `RGB` | 0,0,0 | `` |  |

### `ShowConsoleMsg` â€” 1 claves

`reaper.ShowConsoleMsg(string msg)`

| clave | que es | clave | que es |
|---|---|---|---|
| `SHOW` | " and text will be added to console without op | `` |  |

### `ShowMessageBox` â€” 1 claves

`integer reaper.ShowMessageBox(string msg, string title, integer type)`

| clave | que es | clave | que es |
|---|---|---|---|
| `RETRYCANCEL` | ret 1=OK,2=CANCEL,3=ABORT,4=RETRY,5=IGNORE,6=Y | `` |  |

### `TimeMap_GetMetronomePattern` â€” 1 claves

`integer retval, string pattern = reaper.TimeMap_GetMetronomePattern(ReaProject proj, number time, string pattern)`

| clave | que es | clave | que es |
|---|---|---|---|
| `SET` | string" with a correctly formed pattern string | `` |  |

## 6. Los 730 nombres, por familia

Para saber si algo existe sin preguntar. `daw_api(query=...)` lo hace por descripcion, esto es por prefijo.

**CSurf** (52) â€” `CSurf_FlushUndo`, `CSurf_GetTouchState`, `CSurf_GoEnd`, `CSurf_GoStart`, `CSurf_NumTracks`, `CSurf_OnArrow`, `CSurf_OnFXChange`, `CSurf_OnFwd`, `CSurf_OnInputMonitorChange`, `CSurf_OnInputMonitorChangeEx`, `CSurf_OnMuteChange`, `CSurf_OnMuteChangeEx`, `CSurf_OnPanChange`, `CSurf_OnPanChangeEx`, `CSurf_OnPause`, `CSurf_OnPlay`, `CSurf_OnPlayRateChange`, `CSurf_OnRecArmChange`, `CSurf_OnRecArmChangeEx`, `CSurf_OnRecord`, `CSurf_OnRecvPanChange`, `CSurf_OnRecvVolumeChange`, `CSurf_OnRew`, `CSurf_OnRewFwd`, `CSurf_OnScroll`, `CSurf_OnSelectedChange`, `CSurf_OnSendPanChange`, `CSurf_OnSendVolumeChange`, `CSurf_OnSoloChange`, `CSurf_OnSoloChangeEx`, `CSurf_OnStop`, `CSurf_OnTempoChange`, `CSurf_OnTrackSelection`, `CSurf_OnVolumeChange`, `CSurf_OnVolumeChangeEx`, `CSurf_OnWidthChange`, `CSurf_OnWidthChangeEx`, `CSurf_OnZoom`, `CSurf_ResetAllCachedVolPanStates`, `CSurf_ScrubAmt`, `CSurf_SetAutoMode`, `CSurf_SetPlayState`, `CSurf_SetRepeatState`, `CSurf_SetSurfaceMute`, `CSurf_SetSurfacePan`, `CSurf_SetSurfaceRecArm`, `CSurf_SetSurfaceSelected`, `CSurf_SetSurfaceSolo`, `CSurf_SetSurfaceVolume`, `CSurf_SetTrackListChange`, `CSurf_TrackFromID`, `CSurf_TrackToID`

**TrackFX** (51) â€” `TrackFX_AddByName`, `TrackFX_CopyToTake`, `TrackFX_CopyToTrack`, `TrackFX_Delete`, `TrackFX_EndParamEdit`, `TrackFX_FormatParamValue`, `TrackFX_FormatParamValueNormalized`, `TrackFX_GetByName`, `TrackFX_GetChainVisible`, `TrackFX_GetCount`, `TrackFX_GetEQ`, `TrackFX_GetEQBandEnabled`, `TrackFX_GetEQParam`, `TrackFX_GetEnabled`, `TrackFX_GetFXGUID`, `TrackFX_GetFXName`, `TrackFX_GetFloatingWindow`, `TrackFX_GetFormattedParamValue`, `TrackFX_GetIOSize`, `TrackFX_GetInstrument`, `TrackFX_GetNamedConfigParm`, `TrackFX_GetNumParams`, `TrackFX_GetOffline`, `TrackFX_GetOpen`, `TrackFX_GetParam`, `TrackFX_GetParamEx`, `TrackFX_GetParamFromIdent`, `TrackFX_GetParamIdent`, `TrackFX_GetParamName`, `TrackFX_GetParamNormalized`, `TrackFX_GetParamSectionName`, `TrackFX_GetParameterStepSizes`, `TrackFX_GetPinMappings`, `TrackFX_GetPreset`, `TrackFX_GetPresetIndex`, `TrackFX_GetRecChainVisible`, `TrackFX_GetRecCount`, `TrackFX_GetUserPresetFilename`, `TrackFX_NavigatePresets`, `TrackFX_SetEQBandEnabled`, `TrackFX_SetEQParam`, `TrackFX_SetEnabled`, `TrackFX_SetNamedConfigParm`, `TrackFX_SetOffline`, `TrackFX_SetOpen`, `TrackFX_SetParam`, `TrackFX_SetParamNormalized`, `TrackFX_SetPinMappings`, `TrackFX_SetPreset`, `TrackFX_SetPresetByIndex`, `TrackFX_Show`

**TakeFX** (43) â€” `TakeFX_AddByName`, `TakeFX_CopyToTake`, `TakeFX_CopyToTrack`, `TakeFX_Delete`, `TakeFX_EndParamEdit`, `TakeFX_FormatParamValue`, `TakeFX_FormatParamValueNormalized`, `TakeFX_GetChainVisible`, `TakeFX_GetCount`, `TakeFX_GetEnabled`, `TakeFX_GetEnvelope`, `TakeFX_GetFXGUID`, `TakeFX_GetFXName`, `TakeFX_GetFloatingWindow`, `TakeFX_GetFormattedParamValue`, `TakeFX_GetIOSize`, `TakeFX_GetNamedConfigParm`, `TakeFX_GetNumParams`, `TakeFX_GetOffline`, `TakeFX_GetOpen`, `TakeFX_GetParam`, `TakeFX_GetParamEx`, `TakeFX_GetParamFromIdent`, `TakeFX_GetParamIdent`, `TakeFX_GetParamName`, `TakeFX_GetParamNormalized`, `TakeFX_GetParamSectionName`, `TakeFX_GetParameterStepSizes`, `TakeFX_GetPinMappings`, `TakeFX_GetPreset`, `TakeFX_GetPresetIndex`, `TakeFX_GetUserPresetFilename`, `TakeFX_NavigatePresets`, `TakeFX_SetEnabled`, `TakeFX_SetNamedConfigParm`, `TakeFX_SetOffline`, `TakeFX_SetOpen`, `TakeFX_SetParam`, `TakeFX_SetParamNormalized`, `TakeFX_SetPinMappings`, `TakeFX_SetPreset`, `TakeFX_SetPresetByIndex`, `TakeFX_Show`

**MIDI** (41) â€” `MIDI_CountEvts`, `MIDI_DeleteCC`, `MIDI_DeleteEvt`, `MIDI_DeleteNote`, `MIDI_DeleteTextSysexEvt`, `MIDI_DisableSort`, `MIDI_EnumSelCC`, `MIDI_EnumSelEvts`, `MIDI_EnumSelNotes`, `MIDI_EnumSelTextSysexEvts`, `MIDI_GetAllEvts`, `MIDI_GetCC`, `MIDI_GetCCShape`, `MIDI_GetEvt`, `MIDI_GetGrid`, `MIDI_GetHash`, `MIDI_GetNote`, `MIDI_GetPPQPosFromProjQN`, `MIDI_GetPPQPosFromProjTime`, `MIDI_GetPPQPos_EndOfMeasure`, `MIDI_GetPPQPos_StartOfMeasure`, `MIDI_GetProjQNFromPPQPos`, `MIDI_GetProjTimeFromPPQPos`, `MIDI_GetRecentInputEvent`, `MIDI_GetScale`, `MIDI_GetTextSysexEvt`, `MIDI_GetTrackHash`, `MIDI_InsertCC`, `MIDI_InsertEvt`, `MIDI_InsertNote`, `MIDI_InsertTextSysexEvt`, `MIDI_RefreshEditors`, `MIDI_SelectAll`, `MIDI_SetAllEvts`, `MIDI_SetCC`, `MIDI_SetCCShape`, `MIDI_SetEvt`, `MIDI_SetItemExtents`, `MIDI_SetNote`, `MIDI_SetTextSysexEvt`, `MIDI_Sort`

**Undo** (19) â€” `Undo_BeginBlock`, `Undo_BeginBlock2`, `Undo_CanRedo2`, `Undo_CanUndo2`, `Undo_DoRedo2`, `Undo_DoUndo2`, `Undo_EndBlock`, `Undo_EndBlock2`, `Undo_GetCurEntry`, `Undo_GetEntryDesc`, `Undo_GetEntryTime`, `Undo_GetNumEntries`, `Undo_IsEntryAltTree`, `Undo_OnStateChange`, `Undo_OnStateChange2`, `Undo_OnStateChangeEx`, `Undo_OnStateChangeEx2`, `Undo_OnStateChange_Item`, `Undo_SetCurPos`

**PCM** (10) â€” `PCM_Sink_Enum`, `PCM_Sink_GetExtension`, `PCM_Sink_ShowConfig`, `PCM_Source_BuildPeaks`, `PCM_Source_CreateFromFile`, `PCM_Source_CreateFromFileEx`, `PCM_Source_CreateFromType`, `PCM_Source_Destroy`, `PCM_Source_GetPeaks`, `PCM_Source_GetSectionInfo`

**TimeMap** (10) â€” `TimeMap_GetDividedBpmAtTime`, `TimeMap_GetMeasureInfo`, `TimeMap_GetMetronomePattern`, `TimeMap_GetTimeSigAtTime`, `TimeMap_QNToMeasures`, `TimeMap_QNToTime`, `TimeMap_QNToTime_abs`, `TimeMap_curFrameRate`, `TimeMap_timeToQN`, `TimeMap_timeToQN_abs`

**MIDIEditor** (9) â€” `MIDIEditor_EnumTakes`, `MIDIEditor_GetActive`, `MIDIEditor_GetMode`, `MIDIEditor_GetSetting_int`, `MIDIEditor_GetSetting_str`, `MIDIEditor_GetTake`, `MIDIEditor_LastFocused_OnCommand`, `MIDIEditor_OnCommand`, `MIDIEditor_SetSetting_int`

**joystick** (8) â€” `joystick_create`, `joystick_destroy`, `joystick_enum`, `joystick_getaxis`, `joystick_getbuttonmask`, `joystick_getinfo`, `joystick_getpov`, `joystick_update`

**Envelope** (6) â€” `Envelope_Evaluate`, `Envelope_FormatValue`, `Envelope_GetParentTake`, `Envelope_GetParentTrack`, `Envelope_SortPoints`, `Envelope_SortPointsEx`

**Main** (6) â€” `Main_OnCommand`, `Main_OnCommandEx`, `Main_SaveProject`, `Main_SaveProjectEx`, `Main_UpdateLoopInfo`, `Main_openProject`

**TimeMap2** (6) â€” `TimeMap2_GetDividedBpmAtTime`, `TimeMap2_GetNextChangeTime`, `TimeMap2_QNToTime`, `TimeMap2_beatsToTime`, `TimeMap2_timeToBeats`, `TimeMap2_timeToQN`

**GetMediaItemTake** (5) â€” `GetMediaItemTake`, `GetMediaItemTake_Item`, `GetMediaItemTake_Peaks`, `GetMediaItemTake_Source`, `GetMediaItemTake_Track`

**Master** (5) â€” `Master_GetPlayRate`, `Master_GetPlayRateAtTime`, `Master_GetTempo`, `Master_NormalizePlayRate`, `Master_NormalizeTempo`

**ThemeLayout** (5) â€” `ThemeLayout_GetLayout`, `ThemeLayout_GetParameter`, `ThemeLayout_RefreshAll`, `ThemeLayout_SetLayout`, `ThemeLayout_SetParameter`

**Audio** (4) â€” `Audio_Init`, `Audio_IsPreBuffer`, `Audio_IsRunning`, `Audio_Quit`

**GetSet** (3) â€” `GetSet_ArrangeView2`, `GetSet_LoopTimeRange`, `GetSet_LoopTimeRange2`

**format** (3) â€” `format_timestr`, `format_timestr_len`, `format_timestr_pos`

**parse** (3) â€” `parse_timestr`, `parse_timestr_len`, `parse_timestr_pos`

**CrossfadeEditor** (2) â€” `CrossfadeEditor_OnCommand`, `CrossfadeEditor_Show`

**GetMediaItem** (2) â€” `GetMediaItem`, `GetMediaItem_Track`

**GetSetAutomationItemInfo** (2) â€” `GetSetAutomationItemInfo`, `GetSetAutomationItemInfo_String`

**GetSetProjectInfo** (2) â€” `GetSetProjectInfo`, `GetSetProjectInfo_String`

**Track** (2) â€” `Track_GetPeakHoldDB`, `Track_GetPeakInfo`

**TrackList** (2) â€” `TrackList_AdjustWindows`, `TrackList_UpdateAllExternalSurfaces`

**get** (2) â€” `get_config_var_string`, `get_ini_file`

**kbd** (2) â€” `kbd_enumerateActions`, `kbd_getTextFromCmd`

**midi** (2) â€” `midi_init`, `midi_reinit`

**resolve** (2) â€” `resolve_fn`, `resolve_fn2`

**APIExists** (1) â€” `APIExists`

**APITest** (1) â€” `APITest`

**AddMediaItemToTrack** (1) â€” `AddMediaItemToTrack`

**AddProjectMarker** (1) â€” `AddProjectMarker`

**AddProjectMarker2** (1) â€” `AddProjectMarker2`

**AddRegionOrMarker** (1) â€” `AddRegionOrMarker`

**AddRemoveReaScript** (1) â€” `AddRemoveReaScript`

**AddTakeToMediaItem** (1) â€” `AddTakeToMediaItem`

**AddTempoTimeSigMarker** (1) â€” `AddTempoTimeSigMarker`

**AnyTrackSolo** (1) â€” `AnyTrackSolo`

**ApplyNudge** (1) â€” `ApplyNudge`

**ArmCommand** (1) â€” `ArmCommand`

**AudioAccessorStateChanged** (1) â€” `AudioAccessorStateChanged`

**AudioAccessorUpdate** (1) â€” `AudioAccessorUpdate`

**AudioAccessorValidateState** (1) â€” `AudioAccessorValidateState`

**BypassFxAllTracks** (1) â€” `BypassFxAllTracks`

**CalcMediaSrcLoudness** (1) â€” `CalcMediaSrcLoudness`

**CalculateNormalization** (1) â€” `CalculateNormalization`

**ClearAllRecArmed** (1) â€” `ClearAllRecArmed`

**ClearConsole** (1) â€” `ClearConsole`

**ClearPeakCache** (1) â€” `ClearPeakCache`

**ColorFromNative** (1) â€” `ColorFromNative`

**ColorToNative** (1) â€” `ColorToNative`

**CountActionShortcuts** (1) â€” `CountActionShortcuts`

**CountAutomationItems** (1) â€” `CountAutomationItems`

**CountEnvelopePoints** (1) â€” `CountEnvelopePoints`

**CountEnvelopePointsEx** (1) â€” `CountEnvelopePointsEx`

**CountMediaItems** (1) â€” `CountMediaItems`

**CountProjectMarkers** (1) â€” `CountProjectMarkers`

**CountSelectedMediaItems** (1) â€” `CountSelectedMediaItems`

**CountSelectedTracks** (1) â€” `CountSelectedTracks`

**CountSelectedTracks2** (1) â€” `CountSelectedTracks2`

**CountTCPFXParms** (1) â€” `CountTCPFXParms`

**CountTakeEnvelopes** (1) â€” `CountTakeEnvelopes`

**CountTakes** (1) â€” `CountTakes`

**CountTempoTimeSigMarkers** (1) â€” `CountTempoTimeSigMarkers`

**CountTrackEnvelopes** (1) â€” `CountTrackEnvelopes`

**CountTrackMediaItems** (1) â€” `CountTrackMediaItems`

**CountTracks** (1) â€” `CountTracks`

**CreateNewMIDIItemInProj** (1) â€” `CreateNewMIDIItemInProj`

**CreateTakeAudioAccessor** (1) â€” `CreateTakeAudioAccessor`

**CreateTrackAudioAccessor** (1) â€” `CreateTrackAudioAccessor`

**CreateTrackSend** (1) â€” `CreateTrackSend`

**DB2SLIDER** (1) â€” `DB2SLIDER`

**DeleteActionShortcut** (1) â€” `DeleteActionShortcut`

**DeleteEnvelopePointEx** (1) â€” `DeleteEnvelopePointEx`

**DeleteEnvelopePointRange** (1) â€” `DeleteEnvelopePointRange`

**DeleteEnvelopePointRangeEx** (1) â€” `DeleteEnvelopePointRangeEx`

**DeleteExtState** (1) â€” `DeleteExtState`

**DeleteProjectMarker** (1) â€” `DeleteProjectMarker`

**DeleteProjectMarkerByIndex** (1) â€” `DeleteProjectMarkerByIndex`

**DeleteTakeMarker** (1) â€” `DeleteTakeMarker`

**DeleteTakeStretchMarkers** (1) â€” `DeleteTakeStretchMarkers`

**DeleteTempoTimeSigMarker** (1) â€” `DeleteTempoTimeSigMarker`

**DeleteTrack** (1) â€” `DeleteTrack`

**DeleteTrackMediaItem** (1) â€” `DeleteTrackMediaItem`

**DestroyAudioAccessor** (1) â€” `DestroyAudioAccessor`

**DoActionShortcutDialog** (1) â€” `DoActionShortcutDialog`

**Dock** (1) â€” `Dock_UpdateDockID`

**DockGetPosition** (1) â€” `DockGetPosition`

**DockIsChildOfDock** (1) â€” `DockIsChildOfDock`

**DockWindowActivate** (1) â€” `DockWindowActivate`

**DockWindowAdd** (1) â€” `DockWindowAdd`

**DockWindowAddEx** (1) â€” `DockWindowAddEx`

**DockWindowRefresh** (1) â€” `DockWindowRefresh`

**DockWindowRefreshForHWND** (1) â€” `DockWindowRefreshForHWND`

**DockWindowRemove** (1) â€” `DockWindowRemove`

**EditTempoTimeSigMarker** (1) â€” `EditTempoTimeSigMarker`

**EnsureNotCompletelyOffscreen** (1) â€” `EnsureNotCompletelyOffscreen`

**EnumInstalledFX** (1) â€” `EnumInstalledFX`

**EnumPitchShiftModes** (1) â€” `EnumPitchShiftModes`

**EnumPitchShiftSubModes** (1) â€” `EnumPitchShiftSubModes`

**EnumProjExtState** (1) â€” `EnumProjExtState`

**EnumProjectMarkers** (1) â€” `EnumProjectMarkers`

**EnumProjectMarkers2** (1) â€” `EnumProjectMarkers2`

**EnumProjectMarkers3** (1) â€” `EnumProjectMarkers3`

**EnumProjects** (1) â€” `EnumProjects`

**EnumRegionRenderMatrix** (1) â€” `EnumRegionRenderMatrix`

**EnumTrackMIDIProgramNames** (1) â€” `EnumTrackMIDIProgramNames`

**EnumTrackMIDIProgramNamesEx** (1) â€” `EnumTrackMIDIProgramNamesEx`

**EnumerateFiles** (1) â€” `EnumerateFiles`

**EnumerateSubdirectories** (1) â€” `EnumerateSubdirectories`

**ExecProcess** (1) â€” `ExecProcess`

**FindTempoTimeSigMarker** (1) â€” `FindTempoTimeSigMarker`

**GR** (1) â€” `GR_SelectColor`

**GSC** (1) â€” `GSC_mainwnd`

**GetActionShortcutDesc** (1) â€” `GetActionShortcutDesc`

**GetActiveTake** (1) â€” `GetActiveTake`

**GetAllProjectPlayStates** (1) â€” `GetAllProjectPlayStates`

**GetAppVersion** (1) â€” `GetAppVersion`

**GetArmedCommand** (1) â€” `GetArmedCommand`

**GetAudioAccessorEndTime** (1) â€” `GetAudioAccessorEndTime`

**GetAudioAccessorHash** (1) â€” `GetAudioAccessorHash`

**GetAudioAccessorSamples** (1) â€” `GetAudioAccessorSamples`

**GetAudioAccessorStartTime** (1) â€” `GetAudioAccessorStartTime`

**GetAudioDeviceInfo** (1) â€” `GetAudioDeviceInfo`

**GetConfigWantsDock** (1) â€” `GetConfigWantsDock`

**GetCurrentProjectInLoadSave** (1) â€” `GetCurrentProjectInLoadSave`

**GetCursorContext** (1) â€” `GetCursorContext`

**GetCursorContext2** (1) â€” `GetCursorContext2`

**GetCursorPosition** (1) â€” `GetCursorPosition`

**GetCursorPositionEx** (1) â€” `GetCursorPositionEx`

**GetDisplayedMediaItemColor** (1) â€” `GetDisplayedMediaItemColor`

**GetDisplayedMediaItemColor2** (1) â€” `GetDisplayedMediaItemColor2`

**GetEnvelopeInfo** (1) â€” `GetEnvelopeInfo_Value`

**GetEnvelopeName** (1) â€” `GetEnvelopeName`

**GetEnvelopePoint** (1) â€” `GetEnvelopePoint`

**GetEnvelopePointByTime** (1) â€” `GetEnvelopePointByTime`

**GetEnvelopePointByTimeEx** (1) â€” `GetEnvelopePointByTimeEx`

**GetEnvelopePointEx** (1) â€” `GetEnvelopePointEx`

**GetEnvelopeScalingMode** (1) â€” `GetEnvelopeScalingMode`

**GetEnvelopeStateChunk** (1) â€” `GetEnvelopeStateChunk`

**GetEnvelopeUIState** (1) â€” `GetEnvelopeUIState`

**GetExePath** (1) â€” `GetExePath`

**GetExtState** (1) â€” `GetExtState`

**GetFXEnvelope** (1) â€” `GetFXEnvelope`

**GetFocusedFX** (1) â€” `GetFocusedFX`

**GetFocusedFX2** (1) â€” `GetFocusedFX2`

**GetFreeDiskSpaceForRecordPath** (1) â€” `GetFreeDiskSpaceForRecordPath`

**GetGlobalAutomationOverride** (1) â€” `GetGlobalAutomationOverride`

**GetHZoomLevel** (1) â€” `GetHZoomLevel`

**GetInputActivityLevel** (1) â€” `GetInputActivityLevel`

**GetInputChannelName** (1) â€” `GetInputChannelName`

**GetInputOutputLatency** (1) â€” `GetInputOutputLatency`

**GetItemEditingTime2** (1) â€” `GetItemEditingTime2`

**GetItemFromPoint** (1) â€” `GetItemFromPoint`

**GetItemProjectContext** (1) â€” `GetItemProjectContext`

**GetItemStateChunk** (1) â€” `GetItemStateChunk`

**GetLastColorThemeFile** (1) â€” `GetLastColorThemeFile`

**GetLastMarkerAndCurRegion** (1) â€” `GetLastMarkerAndCurRegion`

**GetLastTouchedFX** (1) â€” `GetLastTouchedFX`

**GetLastTouchedTrack** (1) â€” `GetLastTouchedTrack`

**GetMIDIInputName** (1) â€” `GetMIDIInputName`

**GetMIDIInputNameNoAlias** (1) â€” `GetMIDIInputNameNoAlias`

**GetMIDIOutputName** (1) â€” `GetMIDIOutputName`

**GetMIDIOutputNameNoAlias** (1) â€” `GetMIDIOutputNameNoAlias`

**GetMainHwnd** (1) â€” `GetMainHwnd`

**GetMasterMuteSoloFlags** (1) â€” `GetMasterMuteSoloFlags`

**GetMasterTrack** (1) â€” `GetMasterTrack`

**GetMasterTrackVisibility** (1) â€” `GetMasterTrackVisibility`

**GetMaxMidiInputs** (1) â€” `GetMaxMidiInputs`

**GetMaxMidiOutputs** (1) â€” `GetMaxMidiOutputs`

**GetMediaFileMetadata** (1) â€” `GetMediaFileMetadata`

**GetMediaItemInfo** (1) â€” `GetMediaItemInfo_Value`

**GetMediaItemNumTakes** (1) â€” `GetMediaItemNumTakes`

**GetMediaItemTakeByGUID** (1) â€” `GetMediaItemTakeByGUID`

**GetMediaItemTakeInfo** (1) â€” `GetMediaItemTakeInfo_Value`

**GetMediaItemTrack** (1) â€” `GetMediaItemTrack`

**GetMediaSourceFileName** (1) â€” `GetMediaSourceFileName`

**GetMediaSourceLength** (1) â€” `GetMediaSourceLength`

**GetMediaSourceNumChannels** (1) â€” `GetMediaSourceNumChannels`

**GetMediaSourceParent** (1) â€” `GetMediaSourceParent`

**GetMediaSourceSampleRate** (1) â€” `GetMediaSourceSampleRate`

**GetMediaSourceType** (1) â€” `GetMediaSourceType`

**GetMediaTrackInfo** (1) â€” `GetMediaTrackInfo_Value`

**GetMixerScroll** (1) â€” `GetMixerScroll`

**GetMouseModifier** (1) â€” `GetMouseModifier`

**GetMousePosition** (1) â€” `GetMousePosition`

**GetNumAudioInputs** (1) â€” `GetNumAudioInputs`

**GetNumAudioOutputs** (1) â€” `GetNumAudioOutputs`

**GetNumMIDIInputs** (1) â€” `GetNumMIDIInputs`

**GetNumMIDIOutputs** (1) â€” `GetNumMIDIOutputs`

**GetNumRegionsOrMarkers** (1) â€” `GetNumRegionsOrMarkers`

**GetNumTakeMarkers** (1) â€” `GetNumTakeMarkers`

**GetNumTracks** (1) â€” `GetNumTracks`

**GetOS** (1) â€” `GetOS`

**GetOutputChannelName** (1) â€” `GetOutputChannelName`

**GetOutputLatency** (1) â€” `GetOutputLatency`

**GetParentTrack** (1) â€” `GetParentTrack`

**GetPeakFileName** (1) â€” `GetPeakFileName`

**GetPeakFileNameEx** (1) â€” `GetPeakFileNameEx`

**GetPeakFileNameEx2** (1) â€” `GetPeakFileNameEx2`

**GetPlayPosition** (1) â€” `GetPlayPosition`

**GetPlayPosition2** (1) â€” `GetPlayPosition2`

**GetPlayPosition2Ex** (1) â€” `GetPlayPosition2Ex`

**GetPlayPositionEx** (1) â€” `GetPlayPositionEx`

**GetPlayState** (1) â€” `GetPlayState`

**GetPlayStateEx** (1) â€” `GetPlayStateEx`

**GetProjExtState** (1) â€” `GetProjExtState`

**GetProjectLength** (1) â€” `GetProjectLength`

**GetProjectName** (1) â€” `GetProjectName`

**GetProjectPath** (1) â€” `GetProjectPath`

**GetProjectPathEx** (1) â€” `GetProjectPathEx`

**GetProjectStateChangeCount** (1) â€” `GetProjectStateChangeCount`

**GetProjectTimeOffset** (1) â€” `GetProjectTimeOffset`

**GetProjectTimeSignature** (1) â€” `GetProjectTimeSignature`

**GetProjectTimeSignature2** (1) â€” `GetProjectTimeSignature2`

**GetRegionOrMarker** (1) â€” `GetRegionOrMarker`

**GetRegionOrMarkerInfo** (1) â€” `GetRegionOrMarkerInfo_Value`

**GetResourcePath** (1) â€” `GetResourcePath`

**GetSelectedEnvelope** (1) â€” `GetSelectedEnvelope`

**GetSelectedMediaItem** (1) â€” `GetSelectedMediaItem`

**GetSelectedTrack** (1) â€” `GetSelectedTrack`

**GetSelectedTrack2** (1) â€” `GetSelectedTrack2`

**GetSelectedTrackEnvelope** (1) â€” `GetSelectedTrackEnvelope`

**GetSetEnvelopeInfo** (1) â€” `GetSetEnvelopeInfo_String`

**GetSetEnvelopeState** (1) â€” `GetSetEnvelopeState`

**GetSetEnvelopeState2** (1) â€” `GetSetEnvelopeState2`

**GetSetItemState** (1) â€” `GetSetItemState`

**GetSetItemState2** (1) â€” `GetSetItemState2`

**GetSetMediaItemInfo** (1) â€” `GetSetMediaItemInfo_String`

**GetSetMediaItemTakeInfo** (1) â€” `GetSetMediaItemTakeInfo_String`

**GetSetMediaTrackInfo** (1) â€” `GetSetMediaTrackInfo_String`

**GetSetProjectAuthor** (1) â€” `GetSetProjectAuthor`

**GetSetProjectGrid** (1) â€” `GetSetProjectGrid`

**GetSetProjectNotes** (1) â€” `GetSetProjectNotes`

**GetSetRegionOrMarkerInfo** (1) â€” `GetSetRegionOrMarkerInfo_String`

**GetSetRepeat** (1) â€” `GetSetRepeat`

**GetSetRepeatEx** (1) â€” `GetSetRepeatEx`

**GetSetTempoTimeSigMarkerBasis** (1) â€” `GetSetTempoTimeSigMarkerBasis`

**GetSetTempoTimeSigMarkerFlag** (1) â€” `GetSetTempoTimeSigMarkerFlag`

**GetSetTrackGroupMembership** (1) â€” `GetSetTrackGroupMembership`

**GetSetTrackGroupMembershipEx** (1) â€” `GetSetTrackGroupMembershipEx`

**GetSetTrackGroupMembershipHigh** (1) â€” `GetSetTrackGroupMembershipHigh`

**GetSetTrackSendInfo** (1) â€” `GetSetTrackSendInfo_String`

**GetSetTrackState** (1) â€” `GetSetTrackState`

**GetSetTrackState2** (1) â€” `GetSetTrackState2`

**GetSubProjectFromSource** (1) â€” `GetSubProjectFromSource`

**GetTCPFXParm** (1) â€” `GetTCPFXParm`

**GetTake** (1) â€” `GetTake`

**GetTakeEnvelope** (1) â€” `GetTakeEnvelope`

**GetTakeEnvelopeByName** (1) â€” `GetTakeEnvelopeByName`

**GetTakeMarker** (1) â€” `GetTakeMarker`

**GetTakeName** (1) â€” `GetTakeName`

**GetTakeNumStretchMarkers** (1) â€” `GetTakeNumStretchMarkers`

**GetTakeStretchMarker** (1) â€” `GetTakeStretchMarker`

**GetTakeStretchMarkerSlope** (1) â€” `GetTakeStretchMarkerSlope`

**GetTempoMatchPlayRate** (1) â€” `GetTempoMatchPlayRate`

**GetTempoTimeSigMarker** (1) â€” `GetTempoTimeSigMarker`

**GetThemeColor** (1) â€” `GetThemeColor`

**GetThingFromPoint** (1) â€” `GetThingFromPoint`

**GetToggleCommandState** (1) â€” `GetToggleCommandState`

**GetToggleCommandStateEx** (1) â€” `GetToggleCommandStateEx`

**GetTooltipWindow** (1) â€” `GetTooltipWindow`

**GetTouchedOrFocusedFX** (1) â€” `GetTouchedOrFocusedFX`

**GetTrack** (1) â€” `GetTrack`

**GetTrackAutomationMode** (1) â€” `GetTrackAutomationMode`

**GetTrackColor** (1) â€” `GetTrackColor`

**GetTrackDepth** (1) â€” `GetTrackDepth`

**GetTrackEnvelope** (1) â€” `GetTrackEnvelope`

**GetTrackEnvelopeByChunkName** (1) â€” `GetTrackEnvelopeByChunkName`

**GetTrackEnvelopeByName** (1) â€” `GetTrackEnvelopeByName`

**GetTrackFromPoint** (1) â€” `GetTrackFromPoint`

**GetTrackGUID** (1) â€” `GetTrackGUID`

**GetTrackMIDILyrics** (1) â€” `GetTrackMIDILyrics`

**GetTrackMIDINoteName** (1) â€” `GetTrackMIDINoteName`

**GetTrackMIDINoteNameEx** (1) â€” `GetTrackMIDINoteNameEx`

**GetTrackMIDINoteRange** (1) â€” `GetTrackMIDINoteRange`

**GetTrackMediaItem** (1) â€” `GetTrackMediaItem`

**GetTrackName** (1) â€” `GetTrackName`

**GetTrackNumMediaItems** (1) â€” `GetTrackNumMediaItems`

**GetTrackNumSends** (1) â€” `GetTrackNumSends`

**GetTrackReceiveName** (1) â€” `GetTrackReceiveName`

**GetTrackReceiveUIMute** (1) â€” `GetTrackReceiveUIMute`

**GetTrackReceiveUIVolPan** (1) â€” `GetTrackReceiveUIVolPan`

**GetTrackSendInfo** (1) â€” `GetTrackSendInfo_Value`

**GetTrackSendName** (1) â€” `GetTrackSendName`

**GetTrackSendUIMute** (1) â€” `GetTrackSendUIMute`

**GetTrackSendUIVolPan** (1) â€” `GetTrackSendUIVolPan`

**GetTrackState** (1) â€” `GetTrackState`

**GetTrackStateChunk** (1) â€” `GetTrackStateChunk`

**GetTrackUIMute** (1) â€” `GetTrackUIMute`

**GetTrackUIPan** (1) â€” `GetTrackUIPan`

**GetTrackUIVolPan** (1) â€” `GetTrackUIVolPan`

**GetUnderrunTime** (1) â€” `GetUnderrunTime`

**GetUserFileName** (1) â€” `GetUserFileName`

**GetUserFileNameForRead** (1) â€” `GetUserFileNameForRead`

**GetUserInputs** (1) â€” `GetUserInputs`

**GoToMarker** (1) â€” `GoToMarker`

**GoToRegion** (1) â€” `GoToRegion`

**HasExtState** (1) â€” `HasExtState`

**HasTrackMIDIPrograms** (1) â€” `HasTrackMIDIPrograms`

**HasTrackMIDIProgramsEx** (1) â€” `HasTrackMIDIProgramsEx`

**Help** (1) â€” `Help_Set`

**InsertAutomationItem** (1) â€” `InsertAutomationItem`

**InsertEnvelopePoint** (1) â€” `InsertEnvelopePoint`

**InsertEnvelopePointEx** (1) â€” `InsertEnvelopePointEx`

**InsertMedia** (1) â€” `InsertMedia`

**InsertMediaSection** (1) â€” `InsertMediaSection`

**InsertTrackAtIndex** (1) â€” `InsertTrackAtIndex`

**InsertTrackInProject** (1) â€” `InsertTrackInProject`

**IsMediaExtension** (1) â€” `IsMediaExtension`

**IsMediaItemSelected** (1) â€” `IsMediaItemSelected`

**IsProjectDirty** (1) â€” `IsProjectDirty`

**IsTrackSelected** (1) â€” `IsTrackSelected`

**IsTrackVisible** (1) â€” `IsTrackVisible`

**LICE** (1) â€” `LICE_ClipLine`

**LocalizeString** (1) â€” `LocalizeString`

**Loop** (1) â€” `Loop_OnArrow`

**MB** (1) â€” `MB`

**MIDIEditorFlagsForTrack** (1) â€” `MIDIEditorFlagsForTrack`

**MarkProjectDirty** (1) â€” `MarkProjectDirty`

**MarkTrackItemsDirty** (1) â€” `MarkTrackItemsDirty`

**MediaExplorerGetLastPlayedFileInfo** (1) â€” `MediaExplorerGetLastPlayedFileInfo`

**MediaItemDescendsFromTrack** (1) â€” `MediaItemDescendsFromTrack`

**Menu** (1) â€” `Menu_GetHash`

**MoveEditCursor** (1) â€” `MoveEditCursor`

**MoveMediaItemToTrack** (1) â€” `MoveMediaItemToTrack`

**MuteAllTracks** (1) â€” `MuteAllTracks`

**NamedCommandLookup** (1) â€” `NamedCommandLookup`

**OnPauseButton** (1) â€” `OnPauseButton`

**OnPauseButtonEx** (1) â€” `OnPauseButtonEx`

**OnPlayButton** (1) â€” `OnPlayButton`

**OnPlayButtonEx** (1) â€” `OnPlayButtonEx`

**OnStopButton** (1) â€” `OnStopButton`

**OnStopButtonEx** (1) â€” `OnStopButtonEx`

**OpenColorThemeFile** (1) â€” `OpenColorThemeFile`

**OpenMediaExplorer** (1) â€” `OpenMediaExplorer`

**OscLocalMessageToHost** (1) â€” `OscLocalMessageToHost`

**PluginWantsAlwaysRunFx** (1) â€” `PluginWantsAlwaysRunFx`

**PreventUIRefresh** (1) â€” `PreventUIRefresh`

**PromptForAction** (1) â€” `PromptForAction`

**ReaScriptError** (1) â€” `ReaScriptError`

**RecursiveCreateDirectory** (1) â€” `RecursiveCreateDirectory`

**RefreshToolbar** (1) â€” `RefreshToolbar`

**RefreshToolbar2** (1) â€” `RefreshToolbar2`

**RemoveTrackSend** (1) â€” `RemoveTrackSend`

**RenderFileSection** (1) â€” `RenderFileSection`

**ReorderSelectedTracks** (1) â€” `ReorderSelectedTracks`

**Resample** (1) â€” `Resample_EnumModes`

**ResolveWildcards** (1) â€” `ResolveWildcards`

**ReverseNamedCommandLookup** (1) â€” `ReverseNamedCommandLookup`

**SLIDER2DB** (1) â€” `SLIDER2DB`

**ScaleFromEnvelopeMode** (1) â€” `ScaleFromEnvelopeMode`

**ScaleToEnvelopeMode** (1) â€” `ScaleToEnvelopeMode`

**SectionFromUniqueID** (1) â€” `SectionFromUniqueID`

**SelectAllMediaItems** (1) â€” `SelectAllMediaItems`

**SelectProjectInstance** (1) â€” `SelectProjectInstance`

**SendMIDIMessageToHardware** (1) â€” `SendMIDIMessageToHardware`

**SetActiveTake** (1) â€” `SetActiveTake`

**SetAutomationMode** (1) â€” `SetAutomationMode`

**SetCurrentBPM** (1) â€” `SetCurrentBPM`

**SetCursorContext** (1) â€” `SetCursorContext`

**SetEditCurPos** (1) â€” `SetEditCurPos`

**SetEditCurPos2** (1) â€” `SetEditCurPos2`

**SetEnvelopePoint** (1) â€” `SetEnvelopePoint`

**SetEnvelopePointEx** (1) â€” `SetEnvelopePointEx`

**SetEnvelopeStateChunk** (1) â€” `SetEnvelopeStateChunk`

**SetExtState** (1) â€” `SetExtState`

**SetGlobalAutomationOverride** (1) â€” `SetGlobalAutomationOverride`

**SetItemStateChunk** (1) â€” `SetItemStateChunk`

**SetMIDIEditorGrid** (1) â€” `SetMIDIEditorGrid`

**SetMasterTrackVisibility** (1) â€” `SetMasterTrackVisibility`

**SetMediaItemInfo** (1) â€” `SetMediaItemInfo_Value`

**SetMediaItemLength** (1) â€” `SetMediaItemLength`

**SetMediaItemPosition** (1) â€” `SetMediaItemPosition`

**SetMediaItemSelected** (1) â€” `SetMediaItemSelected`

**SetMediaItemTake** (1) â€” `SetMediaItemTake_Source`

**SetMediaItemTakeInfo** (1) â€” `SetMediaItemTakeInfo_Value`

**SetMediaTrackInfo** (1) â€” `SetMediaTrackInfo_Value`

**SetMixerScroll** (1) â€” `SetMixerScroll`

**SetMouseModifier** (1) â€” `SetMouseModifier`

**SetOnlyTrackSelected** (1) â€” `SetOnlyTrackSelected`

**SetProjExtState** (1) â€” `SetProjExtState`

**SetProjectGrid** (1) â€” `SetProjectGrid`

**SetProjectMarker** (1) â€” `SetProjectMarker`

**SetProjectMarker2** (1) â€” `SetProjectMarker2`

**SetProjectMarker3** (1) â€” `SetProjectMarker3`

**SetProjectMarker4** (1) â€” `SetProjectMarker4`

**SetProjectMarkerByIndex** (1) â€” `SetProjectMarkerByIndex`

**SetProjectMarkerByIndex2** (1) â€” `SetProjectMarkerByIndex2`

**SetRegionOrMarkerInfo** (1) â€” `SetRegionOrMarkerInfo_Value`

**SetRegionRenderMatrix** (1) â€” `SetRegionRenderMatrix`

**SetTakeMarker** (1) â€” `SetTakeMarker`

**SetTakeStretchMarker** (1) â€” `SetTakeStretchMarker`

**SetTakeStretchMarkerSlope** (1) â€” `SetTakeStretchMarkerSlope`

**SetTempoTimeSigMarker** (1) â€” `SetTempoTimeSigMarker`

**SetThemeColor** (1) â€” `SetThemeColor`

**SetToggleCommandState** (1) â€” `SetToggleCommandState`

**SetTrackAutomationMode** (1) â€” `SetTrackAutomationMode`

**SetTrackColor** (1) â€” `SetTrackColor`

**SetTrackMIDILyrics** (1) â€” `SetTrackMIDILyrics`

**SetTrackMIDINoteName** (1) â€” `SetTrackMIDINoteName`

**SetTrackMIDINoteNameEx** (1) â€” `SetTrackMIDINoteNameEx`

**SetTrackSelected** (1) â€” `SetTrackSelected`

**SetTrackSendInfo** (1) â€” `SetTrackSendInfo_Value`

**SetTrackSendUIPan** (1) â€” `SetTrackSendUIPan`

**SetTrackSendUIVol** (1) â€” `SetTrackSendUIVol`

**SetTrackStateChunk** (1) â€” `SetTrackStateChunk`

**SetTrackUIInputMonitor** (1) â€” `SetTrackUIInputMonitor`

**SetTrackUIMute** (1) â€” `SetTrackUIMute`

**SetTrackUIPan** (1) â€” `SetTrackUIPan`

**SetTrackUIPolarity** (1) â€” `SetTrackUIPolarity`

**SetTrackUIRecArm** (1) â€” `SetTrackUIRecArm`

**SetTrackUISolo** (1) â€” `SetTrackUISolo`

**SetTrackUIVolume** (1) â€” `SetTrackUIVolume`

**SetTrackUIWidth** (1) â€” `SetTrackUIWidth`

**ShowActionList** (1) â€” `ShowActionList`

**ShowConsoleMsg** (1) â€” `ShowConsoleMsg`

**ShowMessageBox** (1) â€” `ShowMessageBox`

**ShowPopupMenu** (1) â€” `ShowPopupMenu`

**SnapToGrid** (1) â€” `SnapToGrid`

**SoloAllTracks** (1) â€” `SoloAllTracks`

**Splash** (1) â€” `Splash_GetWnd`

**SplitMediaItem** (1) â€” `SplitMediaItem`

**StuffMIDIMessage** (1) â€” `StuffMIDIMessage`

**TakeIsMIDI** (1) â€” `TakeIsMIDI`

**ToggleTrackSendUIMute** (1) â€” `ToggleTrackSendUIMute`

**TrackCtl** (1) â€” `TrackCtl_SetToolTip`

**UpdateArrange** (1) â€” `UpdateArrange`

**UpdateItemInProject** (1) â€” `UpdateItemInProject`

**UpdateItemLanes** (1) â€” `UpdateItemLanes`

**UpdateTimeline** (1) â€” `UpdateTimeline`

**ValidatePtr** (1) â€” `ValidatePtr`

**ValidatePtr2** (1) â€” `ValidatePtr2`

**ViewPrefs** (1) â€” `ViewPrefs`

**adjustZoom** (1) â€” `adjustZoom`

**file** (1) â€” `file_exists`

**genGuid** (1) â€” `genGuid`

**guidToString** (1) â€” `guidToString`

**image** (1) â€” `image_resolve_fn`

**mkpanstr** (1) â€” `mkpanstr`

**mkvolpanstr** (1) â€” `mkvolpanstr`

**mkvolstr** (1) â€” `mkvolstr`

**my** (1) â€” `my_getViewport`

**parsepanstr** (1) â€” `parsepanstr`

**reduce** (1) â€” `reduce_open_files`

**relative** (1) â€” `relative_fn`

**set** (1) â€” `set_config_var_string`

**stringToGuid** (1) â€” `stringToGuid`

**time** (1) â€” `time_precise`

