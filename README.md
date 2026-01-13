# Intouch2-MQTT
This project is intended to bridge Intouch2 spas with Home Assistant. Unlike
similar projects, this one does not come with batteries included. To integrate
this with your spa, you have to do it yourself. You can do so by forwarding and
sniffing packages from home assistant to figure out what values change.

The way it works is that you read and write a raw memory blob. I have no idea
about how (or if) there is security (or even safety) checks implemented, so be
careful here, and know that **anything you do is at your own risk**. When you
start and stop pumps, or change light status or colors, the spa will push an
update of the affected memory area. You can then set up rules to map it against
MQTT.

What you do is basically that you set up your MQTT values, one MQTT discovery
object per entity in `entities_json`, and you pretty much type it exactly as
you would with static data, with the exception of special `state` and `command`
objects, as described below. For example, if you want to set up a pump you can
check [the Home Assistant documentation for MQTT
fans](https://www.home-assistant.io/integrations/fan.mqtt/), and if you want to
set up a light you simply set it up as described in [the Home Assistant
documentation for MQTT
lights](https://www.home-assistant.io/integrations/light.mqtt/). This program
will change the special objects into MQTT addresses, and handle the updates to
and from other MQTT clients, such as Home Assistant.

## Example rule:
This is the configuration I'm using:
```yaml
spa_target: 192.168.0.123:10022
spa_id: my_wet_hideaway
spa_memory_size: 637 # This is likely not the same as yours
log_level: Info
# # You can define an MQTT target, but you don't have to if you're running as a
# # Home Assistant addon, as the addon will get this information from Home
# # Assistant.
# mqtt_target: core-mosquitto:1883
# mqtt_username: addons
# mqtt_password: 12345 # That's the stupidest combination I've ever heard in my life! That's the kind of thing an idiot would have on his luggage!
entities_json:
  - |-
    {"name": "Primary",
     "unique_id": "spa_light1",
     "type": "light",
     "optimistic": false,
     "availability_topic": "intouch2/availability",
     "command_topic": "/dev/null",
     "state_topic": {"state": {"u8_addr": 601}},
     "state_value_template": "{% if value == \"0\" %}OFF{% else %}ON{% endif %}",
     "rgb_state_topic": {"state": {"addr": 604, "len": 3}},
     "rgb_command_topic": "/dev/null",
     "rgb_value_template": "{{ value_json[0] }},{{ value_json[1] }},{{ value_json[2] }}",
     "effect_state_topic": {"state": {"u8_addr": 601}},
     "effect_value_template": "{% if value == '1' %}Slow Fade{% elif value == '2' %}Fast Fade{% else %}null{% endif %}",
     "effect_command_topic": "/dev/null",
     "effect_list": ["Slow Fade", "Fast Fade"],
     "color_mode": "rgb"
    }
  - |-
    {"name": "Secondary",
     "unique_id": "spa_light2",
     "type": "light",
     "optimistic": false,
     "availability_topic": "intouch2/availability",
     "command_topic": "/dev/null",
     "state_topic": {"state": {"u8_addr": 608}},
     "state_value_template": "{% if value == '0' %}OFF{% else %}ON{% endif %}",
     "rgb_state_topic": {"state": {"addr": 611, "len": 3}},
     "rgb_command_topic": "/dev/null",
     "rgb_value_template": "{{ value_json[0] }},{{ value_json[1] }},{{ value_json[2] }}",
     "effect_state_topic": {"state": {"u8_addr": 608}},
     "effect_value_template": "{% if value == '1' %}Slow Fade{% elif value == '2' %}Fast Fade{% else %}null{% endif %}",
     "effect_command_topic": "/dev/null",
     "effect_list": ["Slow Fade", "Fast Fade"],
     "color_mode": "rgb"
    }
  - |-
    {"name": "Fountain",
     "unique_id": "spa_fountain",
     "type": "fan",
     "optimistic": false,
     "availability_topic": "intouch2/availability",
     "icon": "mdi:fountain",
     "command_topic": {"command": {"key": 23, "pack_type": 10}},
     "command_template": "{% if (value | lower) != (this.state | lower) %}1{% else %}0{% endif %}",
     "state_topic": {"state": {"u8_addr": 363}},
     "state_value_template": "{% if value == '0' %}OFF{% else %}ON{% endif %}"
    }
  - |-
    {"name": "Pump 1",
     "unique_id": "spa_pump1",
     "type": "fan",
     "optimistic": false,
     "availability_topic": "intouch2/availability",
     "command_topic": "/dev/null",
     "state_topic": {"state": {"u8_addr": 261}},
     "state_value_template": "{% if value | int | bitwise_and(3) == 0 %}OFF{% else %}ON{% endif %}",
     "percentage_command_topic": {"command": {"key": 1, "pack_type": 10}},
     "percentage_command_template": "{% set previous = ((this.attributes.percentage | int) / (this.attributes.percentage_step | int)) | int %}{% set new = value | int %}{{ (new - previous) % 3 | int }}",
     "percentage_state_topic": {"state": {"u8_addr": 261}},
     "percentage_value_template": "{% set pump_value = value_json | int | bitwise_and(3) %}{% if pump_value == 0 %}0{% elif pump_value == 2 %}1{% else %}2{% endif %}",
     "speed_range_max": 2
    }
  - |-
    {"name": "Pump 2",
     "unique_id": "spa_pump2",
     "type": "fan",
     "optimistic": false,
     "availability_topic": "intouch2/availability",
     "command_topic": {"command": {"key": 2, "pack_type": 10}},
     "command_template": "{% if (value | lower) != (this.state | lower) %}1{% else %}0{% endif %}",
     "state_topic": {"state": {"u8_addr": 261}},
     "state_value_template": "{% if value | int | bitwise_and(4) == 0 %}OFF{% else %}ON{% endif %}"
    }
  - |-
    {"name": "Pump 3",
     "unique_id": "spa_pump3",
     "type": "fan",
     "optimistic": false,
     "availability_topic": "intouch2/availability",
     "command_topic": {"command": {"key": 3, "pack_type": 10}},
     "command_template": "{% if (value | lower) != (this.state | lower) %}1{% else %}0{% endif %}",
     "state_topic": {"state": {"u8_addr": 261}},
     "state_value_template": "{% if value | int | bitwise_and(16) == 0 %}OFF{% else %}ON{% endif %}"
    }
  - |-
    {"name": "Water",
     "unique_id": "spa_water_climate",
     "type": "climate",
     "optimistic": false,
     "availability_topic": "intouch2/availability",
     "icon": "mdi:thermometer-water",
     "temperature_high_state_topic": {"state": {"u16_addr": 1}},
     "temperature_high_state_template": "{{ (value | float) / 18.0 }}",
     "temperature_high_command_topic": {"command": {"u16_addr": 1, "config_version": 62, "pack_type": 10, "log_version": 62}},
     "temperature_high_command_template": "{{ (value * 18) | int }}",
     "temperature_low_state_topic": {"state": {"u16_addr": 275}},
     "temperature_low_state_template": "{{ (value | float) / 18.0 }}",
     "current_temperature_topic": {"state": {"u16_addr": 277}},
     "current_temperature_template": "{{ (value | float) / 18.0 }}",
     "max_temp": 40,
     "min_temp": 15,
     "precision": 0.5,
     "temp_step": 0.5,
     "preset_mode_state_topic": {"state": "watercare_mode"},
     "preset_mode_value_template": "{% set modes = ['Away From Home', None, 'Energy Saving', 'Super Energy Saving', 'Weekender'] %}{{ modes[value | int] }}",
     "preset_mode_command_topic": {"command": "watercare_mode"},
     "preset_mode_command_template": "{% set modes = ['Away From Home', 'none', 'Energy Saving', 'Super Energy Saving', 'Weekender'] %}{{ modes.index(value) }}",
     "preset_modes": ["Away From Home", "Energy Saving", "Super Energy Saving", "Weekender"],
     "action_topic": {"state": {"u8_addr": 279}},
     "action_template": "{% if value | int == 2 %}heating{% else %}off{% endif %}",
     "modes": ["auto", "heat"],
     "mode_state_topic": "intouch2-loopback/spadet/water/mode",
     "mode_command_topic": "intouch2-loopback/spadet/water/mode"
    }
  - |-
    {"name": "Power usage",
     "unique_id": "spa_power_usage",
     "type": "sensor",
     "device_class": "power",
     "availability_topic": "intouch2/availability",
     "state_topic": {"state": [{"u8_addr": 279}, {"u8_addr": 261}]},
     "value_template": "{% set heater_usage = (value_json[0] | int) * (3.4/2) %}{% set pump1_usage = ((value_json[1] | int | bitwise_and(2))/2) + (value_json[1] | int | bitwise_and(1)) * 1.5 %}{% set pump2_usage = ((value_json[1] | int | bitwise_and(4))/4) * 1.5 %}{% set pump3_usage = ((value_json[1] | int | bitwise_and(16))/16) * 1.5 %}{{ heater_usage + pump1_usage + pump2_usage + pump3_usage }}",
     "unit_of_measurement": "kW"
    }
  - |-
    {"name": "Reset Change Water",
     "unique_id": "spa_reset_reminder_change_water",
     "type": "button",
     "device_class": "restart",
     "availability": [
       {"topic": "intouch2-loopback/reset_buttons/availability",
        "value_template": "{{ 'offline' if value != 'online' else 'online' }}"
       },
       {"topic": {"state": "reminders"},
        "value_template": "{{ 'offline' if value_json is none else 'online' }}"
       },
       {"topic": "intouch2/availability"}
     ],
     "command_topic": {"command": "reminders"},
     "command_template": "{{ {'ChangeWater': 120 } | tojson }}"
    }
  - |-
    {"name": "Reset Rinse Filter",
     "unique_id": "spa_reset_reminder_rinse_filter",
     "type": "button",
     "device_class": "restart",
     "availability": [
       {"topic": "intouch2-loopback/reset_buttons/availability",
        "value_template": "{{ 'offline' if value != 'online' else 'online' }}"
       },
       {"topic": {"state": "reminders"},
        "value_template": "{{ 'offline' if value_json is none else 'online' }}"
       },
       {"topic": "intouch2/availability"}
     ],
     "command_topic": {"command": "reminders"},
     "command_template": "{{ {'RinseFilter': 30 } | tojson }}"
    }
  - |-
    {"name": "Reset Clean Filter",
     "unique_id": "spa_reset_reminder_clean_filter",
     "type": "button",
     "device_class": "restart",
     "availability": [
       {"topic": "intouch2-loopback/reset_buttons/availability",
        "value_template": "{{ 'offline' if value != 'online' else 'online' }}"
       },
       {"topic": {"state": "reminders"},
        "value_template": "{{ 'offline' if value_json is none else 'online' }}"
       },
       {"topic": "intouch2/availability"}
     ],
     "command_topic": {"command": "reminders"},
     "command_template": "{{ {'CleanFilter': 60 } | tojson }}"
    }
  - |-
    {"name": "Allow Reminder Resets",
     "unique_id": "spa_allow_reset_reminder",
     "type": "switch",
     "availability_topic": "intouch2/availability",
     "command_topic": "intouch2-loopback/reset_buttons/availability",
     "state_topic": "intouch2-loopback/reset_buttons/availability",
     "payload_on": "online",
     "payload_off": "offline",
     "value_template": "{{ 'offline' if value != 'online' else 'online' }}"
    }
  - |-
    {"name": "Change Water",
     "unique_id": "spa_reminder_change_water",
     "type": "sensor",
     "availability": [
       {"topic": "intouch2/availability"},
       {"topic": {"state": "reminders"},
        "value_template": "{{ 'offline' if value_json is none else 'online' }}"
       }
     ],
     "device_class": "duration",
     "state_topic": {"state": "reminders"},
     "value_template": "{{ value_json['ChangeWater'] }}"
    }
  - |-
    {"name": "Rinse Filter",
     "unique_id": "spa_reminder_rinse_filter",
     "type": "sensor",
     "device_class": "duration",
     "availability": [
       {"topic": "intouch2/availability"},
       {"topic": {"state": "reminders"},
        "value_template": "{{ 'offline' if value_json is none else 'online' }}"
       }
     ],
     "state_topic": {"state": "reminders"},
     "value_template": "{{ value_json['RinseFilter'] }}"
    }
  - |-
    {"name": "Clean Filter",
     "unique_id": "spa_reminder_clean_filter",
     "type": "sensor",
     "device_class": "duration",
     "optimistic": false,
     "availability": [
       {"topic": "intouch2/availability"},
       {"topic": {"state": "reminders"},
        "value_template": "{{ 'offline' if value_json is none else 'online' }}"
       }
     ],
     "state_topic": {"state": "reminders"},
     "value_template": "{{ value_json['CleanFilter'] }}"
    }
mqtt_availability_topic: availability
mqtt_base_topic: intouch2
package_dump_mqtt_topic: package_dump
spa_forward_listen_ip: 0.0.0.0
dump_traffic: true # For debugging and finding the correct key codes and addresses.
```
As you can see above, you have three choices of values in the json. You can
either have a string, or you can have a state object, or you can have a
command.

All of these will require some data transformation first, which is done with templating.

### State object
The state object can be either the special string `"watercare_mode"`, or one or
more addresses. The type refers to the data at that address, so `{"u8_addr":
279}` will update when the single byte on 279 changes. All of these
alternatives are used in the example above.

### Command object
The command object can either be the special string `"watercare_mode"`, a
single address (for raw memory writes), or a keypress. I don't know what all
the fields in the raw write mode actually mean, but since they are static it
hasn't been an issue.

The raw writes expect to get a raw bytestring matching the size of the chosen
type, so one byte for a `u8` and two for `u16`.

The key presses expect an int (but will accept and strip the suffix `.0`, so a
whole number float will be accepted) which represents how many times the button
should be pressed. This means that the template may return `0` if the state is
not to be changed. In the example above, pump 1 is a two step pump, which is
handled by pressing the button 0-2 times whenever a certain speed is requested.

## Usage
To use this package, you will have to add [https://github.com/Ofenhed/hassio-addons](https://github.com/Ofenhed/hassio-addons) as a addon source in Home Assistant, and install the addon.
