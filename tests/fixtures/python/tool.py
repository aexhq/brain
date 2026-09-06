import json
from wit_world import WitWorld
from wit_world.imports import host


class WitWorld(WitWorld):
    def run(self, input):
        value = json.loads(input.input_json)
        sequence = host.emit("python_tool_ran", json.dumps(value))
        return json.dumps({"echo": value, "sequence": sequence})
