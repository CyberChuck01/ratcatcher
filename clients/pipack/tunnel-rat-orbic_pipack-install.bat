@echo off
rem Tunnel R.A.T. Orbic: copy the PiPack link files to the PiPack and run its setup.
rem Your PC must reach the PiPack: join its "PiPack" hotspot (it is 10.3.141.1 there),
rem or pass another address:  tunnel-rat-orbic_pipack-install.bat dragon@192.168.x.x
title Tunnel R.A.T. Orbic - PiPack setup
cd /d "%~dp0"
set "PIPACK=dragon@10.3.141.1"
if not "%~1"=="" set "PIPACK=%~1"
echo Copying the link files to %PIPACK% (type the PiPack password if asked)...
scp tunnel-rat-orbic_pipack-link.py tunnel-rat-orbic_pipack-setup.sh %PIPACK%:
if errorlevel 1 goto failed
echo.
echo Running the setup on the PiPack (PiPack password again, then sudo)...
ssh -t %PIPACK% "mkdir -p ~/tunnel-rat-orbic && mv -f ~/tunnel-rat-orbic_pipack-link.py ~/tunnel-rat-orbic_pipack-setup.sh ~/tunnel-rat-orbic/ && sudo bash ~/tunnel-rat-orbic/tunnel-rat-orbic_pipack-setup.sh"
if errorlevel 1 goto failed
echo.
echo Finished.
pause
exit /b 0
:failed
echo.
echo Something went wrong - see the messages above.
pause
exit /b 1
