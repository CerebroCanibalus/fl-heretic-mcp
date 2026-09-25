#!/usr/bin/env python3
"""Documenta en AGENTS.md los hallazgos de gestion de proyecto."""
import io

P = r"D:\Mis Juegos\ClaudeMCPs\FLHereticMCP\AGENTS.md"
s = io.open(P, encoding="utf-8").read()

seccion = '''
---

## 15. GESTION DE PROYECTO EN FL (medido, no supuesto)

### La API de proyecto de FL no existe

`dir(general)` no tiene `saveProject`, ni `getProjectFilePath`, ni
`getProjectName`, ni `newProject`. Comprobado sobre los 909 simbolos de los
11 modulos. Lo unico de guardado es `general.saveUndo()`, que es un punto de
undo, no a disco.

De las **79** constantes `midi.FPT_*` solo hay `FPT_Save` (Ctrl+S) y
`FPT_SaveNew`. **No existe `FPT_New`, ni `FPT_Open`, ni `FPT_Close`.**

### FPT_Save SI funciona (verificado)

Sobre un proyecto **con ruta**, `transport.globalTransport(midi.FPT_Save, 1)`
guarda en disco sin abrir dialogo. Prueba:

| | Antes | Despues |
|---|---|---|
| SHA-256 del .flp | 91ea14e4 | e0dbb294 |
| mtime | 16:23:02 | 16:31:55 |
| `general.getChangedFlag()` | 1 | 0 |

Latencia: ~90 ms.

Ojo: `has_file` de `project.metadata` devuelve `false` incluso con el
proyecto abierto desde una ruta, porque se basa en `getProjectTitle()`, que
FL no rellena. **No se puede detectar por la API si un proyecto tiene
ruta**; hay que deducirlo de la ventana. Aun asi, `FPT_Save` funciona: FL
si conoce su propia ruta.

### El dialogo de "Save as" NO se puede automatizar

Cuatro vias, todas fallidas contra FL Studio 2025 real:

1. **Ctrl+Shift+S**: no es el atajo de "Save as". El dialogo nunca aparece.
2. **`FPT_Menu`**: abre el menu rapido contextual (`TQuickPopupMenuWindow`),
   no el menu File. Recorriendo las 13 posiciones con `FPT_Down` + `FPT_Enter`
   no aparece ningun dialogo de guardado.
3. **`FPT_SaveNew`**: **si** abre el dialogo la primera vez
   (`TNewProjForm`, titulo "Save as"). Sus campos son `TQuickEdit`,
   controles Delphi internos. Aceptan `WM_SETTEXT`, pero al confirmar con
   Enter el dialogo se cierra **sin guardar**: el titulo de la ventana pasa
   a "Project_3.flp" y no existe ningun `Project_*.flp` en el disco. En el
   segundo intento `FPT_SaveNew` deja de abrir el dialogo porque el proyecto
   ya tiene nombre en memoria.
4. **Leer la barra de menus por Win32**: el `TNewMenu` de la ventana
   principal existe y `GetMenu` devuelve un handle, pero `GetMenuItemCount`
   da 0: es un menu **owner-draw** (los items se dibujan a mano, no son items
   de Win32). No se pueden leer sus textos.

Conclusion: la UI de FL resiste la automatizacion. Un `.flp` es un fichero, y
ahi si se puede trabajar.

### Lo que si funciona: crear por copia de plantilla

Un `.flp` es un formato binario propio (cabecera `FLhd`, no es un ZIP).
`new-project` copia una plantilla a la ruta destino y opcionalmente lanza
FL con ella. Sin teclado, sin dialogos, sin robar el foco.

### Plantillas disponibles

| Fichero | Bytes | Contenido |
|---|---|---|
| `TemplateProject.flp` | 53.381 | 5 canales: 4 "808" **sin instrumento** (type 0) + FLEX Bass (type 2). 0 patrones, 0 notas. |
| `emptyProject.flp` | 46.347 | Realmente vacio: 1 canal "Sampler" (el que FL crea solo), 0 patrones, tempo 140. |

Las dos estan en `D:\\Mis Juegos\\ClaudeMCPs\\FLStudioMCP\\TemplateProject\\`.

### Trampa de PowerShell

`Start-Process -FilePath FL64.exe -ArgumentList "C:\\ruta\\con espacios\\p.flp"`
parte la ruta y FL responde "The file Studio\\Projects\\p.flp could not be
found". El codigo propio usa `Command::arg()`, que en Windows entrecomilla
solo, asi que `fl-heretic open <ruta>` no tiene ese problema.

### Comandos

```bash
fl-heretic config template <ruta.flp>     # fija la plantilla de los nuevos
fl-heretic new-project <nombre>           # crea y abre
fl-heretic new-project <nombre> --no-open # solo crea el fichero
fl-heretic open <ruta.flp>                # abre un .flp existente
```

'''

s = s.rstrip() + "\n" + seccion
io.open(P, "w", encoding="utf-8", newline="\n").write(s)
print("AGENTS.md: seccion 15 anadida")
