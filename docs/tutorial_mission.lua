-- Zartex'27 tutorial mission
-- Cinematic intro, get into the car, drive from point A to point B.
-- Every call below is a function of the Lua mission API (MISSION.*).

---------------------------------------------------------------------
-- 1. Where and when
---------------------------------------------------------------------
MISSION.missionSummary.Level = "Miami_Day"     -- city and time of day
MISSION.missionSummary.MoodId = 33             -- weather, sky and light (Moods\mood33.txt)

-- Point A (start, on foot) and point B (destination), in metres: x, height, z.
-- They are two points on the streets of downtown Miami that a stock mission already uses.
local A = { -836.87, 0.52, 1486.32 }
local B = { -1067.45, 0.52, 997.13 }

---------------------------------------------------------------------
-- 2. Actors: the player, the car, the cameras, the destination marker
---------------------------------------------------------------------
-- role 1 = the player; this also sets the mission's start position
local Player = MISSION.CreateCharacter(A[1] + 4, A[2], A[3], 0, SKIN_TANNER, 1, "Player", -1, -1, 1.0, 0.0, 0, 1.0)

-- vehicle id 6 (the Mustang of the stock races), parked 6 m from the player
local Car = MISSION.CreateVehicle(A[1] - 2, A[2], A[3], 0, 0x6, 0, "Car")

-- two fixed cameras for the opening shot: (camera x,y,z) looking at (target x,y,z)
local Cam1 = MISSION.CreateCameraLookingAtPosition(A[1] + 30, A[2] + 14, A[3] + 40,  A[1], A[2] + 1, A[3], 1, 1, 0, nil, "Intro camera 1")
local Cam2 = MISSION.CreateCameraLookingAtPosition(A[1] - 18, A[2] + 4,  A[3] - 22,  A[1], A[2] + 1, A[3], 1, 1, 0, nil, "Intro camera 2")

-- markers (shown only when their ActorCreation node runs)
local CarIcon  = MISSION.CreateObjectiveIconAttachedTo(Car, 0, 4, 0)
local DestIcon = MISSION.CreateObjectiveIcon(B[1], B[2], B[3], 6)

---------------------------------------------------------------------
-- 3. Logic: everything starts from LogicStart
---------------------------------------------------------------------
local Start = MISSION.LogicStart("Tutorial - Start")

-- cinematic: the in-game cutscene mode hides the HUD and keeps the player from moving
local Igcs = MISSION.ToggleIngameCutscene(true, true)
Start.wireCollection.Add(Igcs)
Start.wireCollection.Add(MISSION.FadeIn(1.0))

-- camera shots: type 1 = a camera actor; duration in seconds, then blend time.
-- A shot starts the next one it is wired to when its duration is over.
local Shot1 = MISSION.CameraSelect(Cam1, 1, 4.0, 0, 2, 1, 0)
local Shot2 = MISSION.CameraSelect(Cam2, 1, 4.0, 1.5, 2, 1, 0)
Start.wireCollection.Add(Shot1)
Shot1.wireCollection.Add(Shot2)

-- the player is on foot: walk to the car and get in
local EnterCar = MISSION.CharacterEnterExitedVehicle(Player, Car)
Shot2.wireCollection.Add(Igcs, 2)                               -- wire type 2 turns the cutscene off
Shot2.wireCollection.Add(MISSION.ActorCreation(CarIcon))        -- show the marker over the car
Shot2.wireCollection.Add(EnterCar)                              -- and start watching

-- player is in the car: swap the markers and watch for point B
local Arrived = MISSION.ActorHasReachedPoint(Player, B[1], B[2], B[3], 12)
EnterCar.wireCollection.Add(MISSION.DestroyActor(CarIcon))
EnterCar.wireCollection.Add(MISSION.ActorCreation(DestIcon))
EnterCar.wireCollection.Add(Arrived)

-- arrived: the marker goes away and the mission is complete
Arrived.wireCollection.Add(MISSION.DestroyActor(DestIcon))
Arrived.wireCollection.Add(MISSION.MissionComplete(false))

-- failures, watched from the start
local Dead = MISSION.CharacterHasDied(Player)
Dead.wireCollection.Add(MISSION.FailMission(0))
Start.wireCollection.Add(Dead)

local Wrecked = MISSION.VehicleWrecked(Car)
Wrecked.wireCollection.Add(MISSION.FailMission(5, 5.0))
Start.wireCollection.Add(Wrecked)
