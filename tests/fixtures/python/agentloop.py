import json
from wit_world import WitWorld
from wit_world.imports import host
from wit_world.imports.types import TurnOutput


class WitWorld(WitWorld):
    def turn(self, input):
        kv = json.loads(input.kv_json)
        kv["calls"] = kv.get("calls", 0) + 1
        host.kv_put("calls", json.dumps(kv["calls"]))
        sequence = host.emit("python_ran", json.dumps({"calls": kv["calls"]}))
        return TurnOutput(json.dumps({"sequence": sequence, "calls": kv["calls"]}))
