# Golden outputs (oracle = original LuaC + MoonSharp, net472)

Gerado por `oracle/out/LuaC.exe`. 12 de 12 scripts compilam.

- Driv3r (.mpc): `LuaC.exe <script> -o golden/<nome>.mpc`
  template, deathmatch, presidentsrun, scriptedTraffic, survival: scripts originais.
  areatest, checkpointRace: usam `oracle/scripts_fixed/` (originais estavam desatualizados:
  faltava o argumento `skin` em CreateCharacter / CreateCharacterInVehicle; inserido SKIN_TANNER).
- Parallel Lines (.sp): `LuaC.exe <script> -dpl -o golden/<nome>.sp`
  O LuaC CLI original nao implementa -dpl (ambos os ramos chamam o loader do Driv3r).
  `oracle/OracleMain.cs` replica o fluxo da GUI (PACKAGE + INITMISSION + Save).
  DPL_template, DPL_ambulance, DPL_ambulance_init, DPL_vehicle, DPLtest.
