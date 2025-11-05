#!/usr/bin/with-contenv bashio

export MQTT_SERVER="$(bashio::services 'mqtt' 'host'):$(bashio::services 'mqtt' 'port')"
echo "Got MQTT server $MQTT_SERVER"
export MQTT_USER="$(bashio::services 'mqtt' 'username')"
echo "Got MQTT username $MQTT_USER"
export MQTT_PASSWORD="$(bashio::services 'mqtt' 'password')"
echo "Got MQTT password"

exec /usr/local/bin/intouch2-mqtt
