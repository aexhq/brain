/* eslint-disable */
/** Generated from Brain-owned v1 contracts. Do not edit. */
import type { JSONSchema } from "zod/v4/core";
export const environmentDefinitions: Record<string, JSONSchema.JSONSchema> = {
  "EnvironmentCommand": {
    "properties": {
      "contract": {
        "const": "environment/v1"
      },
      "operation": {
        "$ref": "#/$defs/EnvironmentOperation"
      }
    },
    "required": [
      "contract",
      "operation"
    ],
    "type": "object"
  },
  "EnvironmentOperation": {
    "description": "One operation on an Environment, named by `(session_id, environment, sequence)`.",
    "properties": {
      "binding": {
        "$ref": "#/$defs/EnvironmentRef"
      },
      "configuration": {
        "default": null
      },
      "context": {
        "$ref": "#/$defs/ExecutionCallback",
        "description": "Controller services are separate from the hosted invocation's callback."
      },
      "environment": {
        "$ref": "#/$defs/EnvironmentName"
      },
      "reporter": {
        "$ref": "#/$defs/ExecutionCallback"
      },
      "request": {
        "$ref": "#/$defs/EnvironmentRequest"
      },
      "sequence": {
        "description": "The sequence of the journal record that started this operation. With\n`session_id` it names the operation: a redelivery carries the same pair, so a\nreceiver that already answered it can say so.",
        "format": "uint64",
        "minimum": 1,
        "type": "integer"
      },
      "session_id": {
        "$ref": "#/$defs/SessionId"
      },
      "template": {
        "$ref": "#/$defs/EnvironmentName"
      }
    },
    "required": [
      "sequence",
      "environment",
      "session_id",
      "request"
    ],
    "type": "object"
  },
  "EnvironmentRef": {
    "additionalProperties": false,
    "properties": {
      "name": {
        "$ref": "#/$defs/EnvironmentName"
      },
      "sequence": {
        "format": "uint64",
        "minimum": 0,
        "type": "integer"
      }
    },
    "required": [
      "name",
      "sequence"
    ],
    "type": "object"
  },
  "EnvironmentName": {
    "pattern": "^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$",
    "type": "string"
  },
  "ExecutionCallback": {
    "additionalProperties": false,
    "description": "Where a remote invocation reaches only its caller-granted services.",
    "properties": {
      "methods": {
        "items": {
          "type": "string"
        },
        "type": "array"
      },
      "token": {
        "maxLength": 256,
        "minLength": 1,
        "type": "string"
      },
      "url": {
        "format": "uri",
        "maxLength": 2048,
        "minLength": 1,
        "type": "string"
      }
    },
    "required": [
      "url",
      "token",
      "methods"
    ],
    "type": "object"
  },
  "EnvironmentRequest": {
    "description": "What Brain asks an Environment to do. An Environment provides resources and learns\nwhat runs in it only when asked to run it: setup carries its configuration;\nexecute carries an opaque implementation and input.",
    "oneOf": [
      {
        "properties": {
          "configuration": true,
          "type": {
            "const": "setup",
            "type": "string"
          }
        },
        "required": [
          "type",
          "configuration"
        ],
        "type": "object"
      },
      {
        "properties": {
          "input": true,
          "name": {
            "pattern": "^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$",
            "type": "string"
          },
          "type": {
            "const": "call",
            "type": "string"
          }
        },
        "required": [
          "type",
          "name",
          "input"
        ],
        "type": "object"
      },
      {
        "properties": {
          "callback": {
            "$ref": "#/$defs/ExecutionCallback",
            "description": "Sent only during execution, never journaled."
          },
          "deadline_ms": {
            "format": "uint64",
            "minimum": 0,
            "type": "integer"
          },
          "implementation": {
            "description": "Interpreted only by the Environment; fixes the runtime entrypoint and configuration."
          },
          "input": true,
          "type": {
            "const": "execute",
            "type": "string"
          }
        },
        "required": [
          "type",
          "implementation",
          "input"
        ],
        "type": "object"
      },
      {
        "properties": {
          "target_sequence": {
            "format": "uint64",
            "minimum": 1,
            "type": "integer"
          },
          "type": {
            "const": "cancel",
            "type": "string"
          }
        },
        "required": [
          "type",
          "target_sequence"
        ],
        "type": "object"
      },
      {
        "properties": {
          "type": {
            "const": "detach",
            "type": "string"
          }
        },
        "required": [
          "type"
        ],
        "type": "object"
      },
      {
        "properties": {
          "type": {
            "const": "teardown",
            "type": "string"
          }
        },
        "required": [
          "type"
        ],
        "type": "object"
      }
    ]
  },
  "SessionId": {
    "pattern": "^ses_[A-Za-z0-9]{20,32}$",
    "type": "string"
  },
  "EnvironmentResponse": {
    "properties": {
      "contract": {
        "const": "environment/v1"
      },
      "receipt": {
        "$ref": "#/$defs/EnvironmentReceipt"
      },
      "sequence": {
        "format": "uint64",
        "minimum": 1,
        "type": "integer"
      }
    },
    "required": [
      "contract",
      "sequence",
      "receipt"
    ],
    "type": "object"
  },
  "EnvironmentReceipt": {
    "oneOf": [
      {
        "properties": {
          "on_turn_end": {
            "description": "Setup may register a method Brain calls after each turn, with its start sequence.",
            "pattern": "^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$",
            "type": "string"
          },
          "type": {
            "const": "accepted",
            "type": "string"
          }
        },
        "required": [
          "type"
        ],
        "type": "object"
      },
      {
        "properties": {
          "data": true,
          "type": {
            "const": "progress",
            "type": "string"
          }
        },
        "required": [
          "type",
          "data"
        ],
        "type": "object"
      },
      {
        "properties": {
          "output": true,
          "type": {
            "const": "result",
            "type": "string"
          }
        },
        "required": [
          "type",
          "output"
        ],
        "type": "object"
      },
      {
        "description": "The entrypoint returned; only an explicit finish closes a Tool execution.",
        "properties": {
          "output": true,
          "type": {
            "const": "returned",
            "type": "string"
          }
        },
        "required": [
          "type"
        ],
        "type": "object"
      },
      {
        "properties": {
          "code": {
            "pattern": "^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$",
            "type": "string"
          },
          "details": true,
          "message": {
            "maxLength": 4096,
            "type": "string"
          },
          "retryable": {
            "type": "boolean"
          },
          "type": {
            "const": "failure",
            "type": "string"
          }
        },
        "required": [
          "type",
          "code",
          "message",
          "retryable"
        ],
        "type": "object"
      },
      {
        "properties": {
          "message": {
            "maxLength": 4096,
            "type": "string"
          },
          "type": {
            "const": "unknown",
            "type": "string"
          }
        },
        "required": [
          "type",
          "message"
        ],
        "type": "object"
      }
    ]
  },
  "HarnessInitialize": {
    "additionalProperties": false,
    "properties": {
      "configuration": true,
      "protocol": {
        "type": "string"
      },
      "state_directory": {
        "type": "string"
      }
    },
    "required": [
      "protocol",
      "state_directory",
      "configuration"
    ],
    "type": "object"
  },
  "HarnessSession": {
    "additionalProperties": false,
    "properties": {
      "session_id": {
        "$ref": "#/$defs/SessionId"
      }
    },
    "required": [
      "session_id"
    ],
    "type": "object"
  },
  "HarnessRun": {
    "additionalProperties": false,
    "properties": {
      "input": {
        "$ref": "#/$defs/TurnInput"
      },
      "model": {
        "$ref": "#/$defs/ModelDef"
      },
      "sequence": {
        "format": "uint64",
        "minimum": 0,
        "type": "integer"
      },
      "session_id": {
        "$ref": "#/$defs/SessionId"
      }
    },
    "required": [
      "session_id",
      "sequence",
      "model",
      "input"
    ],
    "type": "object"
  },
  "TurnInput": {
    "description": "What Brain hands the loop for one turn.",
    "properties": {
      "configuration": true,
      "events": {
        "description": "A finite batch after the last successfully completed delivery, oldest first.",
        "items": {
          "$ref": "#/$defs/Event"
        },
        "maxItems": 1000,
        "type": "array"
      },
      "input": {
        "$ref": "#/$defs/UserInput",
        "description": "Absent when committed Tool observations activate the loop without a user message."
      },
      "kv": {
        "additionalProperties": true,
        "description": "Saved loop state. Author bindings expose it through KV services.",
        "type": "object"
      },
      "runtime": {
        "$ref": "#/$defs/RuntimeEnvelope"
      },
      "system": {
        "description": "The system prompt the session was created with. Used on every model call unless\nthe loop sends its own.",
        "type": "string"
      },
      "tools": {
        "description": "The tools the session was created with: offered whole on every model call unless\nthe loop names a subset. Brain admitted and provisioned exactly these.",
        "items": {
          "$ref": "#/$defs/ActivationTool"
        },
        "type": "array"
      },
      "transcript": {
        "description": "The transcript as it stands: what the next model call would see.",
        "items": {
          "$ref": "#/$defs/Message"
        },
        "type": "array"
      }
    },
    "required": [
      "transcript",
      "kv",
      "events",
      "configuration",
      "system",
      "tools",
      "runtime"
    ],
    "type": "object"
  },
  "Event": {
    "description": "One journal record as a client reads it. `(session_id, sequence)` names it; there\nis no other identifier.",
    "properties": {
      "data": true,
      "event_type": {
        "pattern": "^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$",
        "type": "string"
      },
      "origin": {
        "$ref": "#/$defs/EventOrigin",
        "description": "Absent on kernel records and historical extension records without attribution."
      },
      "recorded_at_ms": {
        "format": "uint64",
        "minimum": 0,
        "type": "integer"
      },
      "sequence": {
        "format": "uint64",
        "minimum": 1,
        "type": "integer"
      }
    },
    "required": [
      "sequence",
      "recorded_at_ms",
      "event_type",
      "data"
    ],
    "type": "object"
  },
  "EventOrigin": {
    "oneOf": [
      {
        "additionalProperties": false,
        "properties": {
          "kind": {
            "const": "agentloop",
            "type": "string"
          },
          "sequence": {
            "format": "uint64",
            "minimum": 0,
            "type": "integer"
          }
        },
        "required": [
          "kind",
          "sequence"
        ],
        "type": "object"
      },
      {
        "additionalProperties": false,
        "properties": {
          "kind": {
            "const": "tool",
            "type": "string"
          },
          "sequence": {
            "format": "uint64",
            "minimum": 0,
            "type": "integer"
          }
        },
        "required": [
          "kind",
          "sequence"
        ],
        "type": "object"
      },
      {
        "additionalProperties": false,
        "properties": {
          "environment": {
            "$ref": "#/$defs/EnvironmentRef"
          },
          "kind": {
            "const": "environment",
            "type": "string"
          }
        },
        "required": [
          "kind",
          "environment"
        ],
        "type": "object"
      }
    ]
  },
  "UserInput": {
    "additionalProperties": false,
    "description": "What an application hands a session on `send`.",
    "properties": {
      "media": {
        "items": {
          "$ref": "#/$defs/Media"
        },
        "type": "array"
      },
      "message": {
        "minLength": 1,
        "type": "string"
      }
    },
    "required": [
      "message"
    ],
    "type": "object"
  },
  "Media": {
    "oneOf": [
      {
        "additionalProperties": false,
        "properties": {
          "type": {
            "const": "image",
            "type": "string"
          },
          "url": {
            "type": "string"
          }
        },
        "required": [
          "type",
          "url"
        ],
        "type": "object"
      },
      {
        "additionalProperties": false,
        "properties": {
          "media_type": {
            "$ref": "#/$defs/FileMediaType"
          },
          "type": {
            "const": "file",
            "type": "string"
          },
          "url": {
            "type": "string"
          }
        },
        "required": [
          "type",
          "media_type",
          "url"
        ],
        "type": "object"
      }
    ]
  },
  "FileMediaType": {
    "enum": [
      "application/pdf"
    ],
    "type": "string"
  },
  "RuntimeEnvelope": {
    "properties": {
      "deterministic_seed": {
        "items": {
          "format": "uint8",
          "maximum": 255,
          "minimum": 0,
          "type": "integer"
        },
        "maxItems": 32,
        "minItems": 32,
        "type": "array"
      },
      "logical_time_ms": {
        "format": "uint64",
        "minimum": 0,
        "type": "integer"
      }
    },
    "required": [
      "logical_time_ms",
      "deterministic_seed"
    ],
    "type": "object"
  },
  "ActivationTool": {
    "description": "Definitions and logical choices made available to an Agentloop, without executable descriptors.",
    "properties": {
      "description": {
        "type": "string"
      },
      "environment_refs": {
        "items": {
          "$ref": "#/$defs/EnvironmentRef"
        },
        "type": "array"
      },
      "environments": {
        "items": {
          "$ref": "#/$defs/EnvironmentName"
        },
        "type": "array"
      },
      "input_schema": {
        "type": "object"
      },
      "name": {
        "pattern": "^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$",
        "type": "string"
      },
      "output_schema": {
        "type": "object"
      }
    },
    "required": [
      "name",
      "description",
      "input_schema",
      "environments"
    ],
    "type": "object"
  },
  "Message": {
    "additionalProperties": false,
    "properties": {
      "content": {
        "items": {
          "$ref": "#/$defs/ContentBlock"
        },
        "type": "array"
      },
      "role": {
        "$ref": "#/$defs/Role"
      }
    },
    "required": [
      "role",
      "content"
    ],
    "type": "object"
  },
  "ContentBlock": {
    "oneOf": [
      {
        "additionalProperties": false,
        "properties": {
          "text": {
            "type": "string"
          },
          "type": {
            "const": "text",
            "type": "string"
          }
        },
        "required": [
          "type",
          "text"
        ],
        "type": "object"
      },
      {
        "additionalProperties": false,
        "properties": {
          "type": {
            "const": "image",
            "type": "string"
          },
          "url": {
            "description": "HTTPS URL fetched by the model provider.",
            "type": "string"
          }
        },
        "required": [
          "type",
          "url"
        ],
        "type": "object"
      },
      {
        "additionalProperties": false,
        "properties": {
          "media_type": {
            "$ref": "#/$defs/FileMediaType"
          },
          "type": {
            "const": "file",
            "type": "string"
          },
          "url": {
            "type": "string"
          }
        },
        "required": [
          "type",
          "media_type",
          "url"
        ],
        "type": "object"
      },
      {
        "additionalProperties": false,
        "properties": {
          "data": true,
          "format": {
            "description": "Versioned adapter format; incompatible adapters must reject the item.",
            "type": "string"
          },
          "type": {
            "const": "native",
            "type": "string"
          }
        },
        "required": [
          "type",
          "format",
          "data"
        ],
        "type": "object"
      },
      {
        "additionalProperties": false,
        "properties": {
          "id": {
            "type": "string"
          },
          "input": true,
          "name": {
            "type": "string"
          },
          "type": {
            "const": "tool_use",
            "type": "string"
          }
        },
        "required": [
          "type",
          "id",
          "name",
          "input"
        ],
        "type": "object"
      },
      {
        "additionalProperties": false,
        "properties": {
          "content": true,
          "is_error": {
            "description": "ALWAYS set on a failed tool. Omitting the flag on a failure lets the\nmodel read that failure as a success.",
            "type": "boolean"
          },
          "media": {
            "description": "Model-visible media alongside the ordinary JSON Tool output.",
            "items": {
              "$ref": "#/$defs/Media"
            },
            "type": "array"
          },
          "tool_use_id": {
            "type": "string"
          },
          "type": {
            "const": "tool_result",
            "type": "string"
          }
        },
        "required": [
          "type",
          "tool_use_id",
          "content",
          "is_error"
        ],
        "type": "object"
      }
    ]
  },
  "Role": {
    "enum": [
      "user",
      "assistant",
      "developer"
    ],
    "type": "string"
  },
  "ModelDef": {
    "additionalProperties": false,
    "properties": {
      "attachment": {
        "type": "boolean"
      },
      "context_window_tokens": {
        "format": "uint64",
        "minimum": 0,
        "type": "integer"
      },
      "cost": {
        "$ref": "#/$defs/ModelCost"
      },
      "id": {
        "type": "string"
      },
      "input_modalities": {
        "items": {
          "type": "string"
        },
        "type": "array"
      },
      "max_output_tokens": {
        "format": "uint64",
        "minimum": 0,
        "type": "integer"
      },
      "output_modalities": {
        "items": {
          "type": "string"
        },
        "type": "array"
      },
      "reasoning": {
        "type": "boolean"
      },
      "structured_output": {
        "type": "boolean"
      },
      "tool_call": {
        "type": "boolean"
      }
    },
    "required": [
      "id"
    ],
    "type": "object"
  },
  "ModelCost": {
    "additionalProperties": false,
    "description": "USD per million tokens.",
    "properties": {
      "cache_read": {
        "format": "double",
        "type": "number"
      },
      "cache_write": {
        "format": "double",
        "type": "number"
      },
      "input": {
        "format": "double",
        "type": "number"
      },
      "output": {
        "format": "double",
        "type": "number"
      }
    },
    "required": [
      "input",
      "output"
    ],
    "type": "object"
  }
};
