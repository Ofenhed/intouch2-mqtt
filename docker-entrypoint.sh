#!/usr/bin/env bashio

export MQTT_SERVER="$(bashio::services 'mqtt' 'host'):$(bashio::services 'mqtt' 'port')"
export MQTT_USER="$(bashio::services 'mqtt' 'username')"
export MQTT_PASSWORD="$(bashio::services 'mqtt' 'password')"

exec /bin/intouch2-mqtt
