@echo off
REM Arranca loopMIDI (si no corre) y el daemon de fl-studio-mcp (TCP 127.0.0.1:9787).
title fl-studio-mcp launcher
tasklist /FI "IMAGENAME eq loopMIDI.exe" 2>nul | find /I "loopMIDI.exe" >nul || start "" "C:\Program Files (x86)\Tobias Erichsen\loopMIDI\loopMIDI.exe"
netstat -ano 2>nul | findstr /C:"127.0.0.1:9787" >nul || start "" wscript.exe "C:\Users\Admin\AppData\Roaming\Python\Python314\Scripts\fl-studio-mcp-daemon.vbs"
echo loopMIDI y daemon activos.