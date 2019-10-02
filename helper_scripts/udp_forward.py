#!/usr/bin/python3

# This scripts simply forwards the UDP broadcast messages the app sends, to allow for a behind-NAT in.touch 2 wireless node, in case you want to partition or firewall your personal network or the node itself.

from scapy.all import *
import sys

destination_ip = sys.argv[1]

with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
    try:
      sock.bind(("", int(sys.argv[2])))
    except socket.error:
        print('Bind failed.')
        sys.exit(1)
    print("Socket activated")
    while 1:
        data, (src_ip, src_port) = sock.recvfrom(4096)
        print(data, src_ip, src_port)

        packet = IP(dst=destination_ip, src=src_ip)/UDP(dport=int(sys.argv[2]), sport=src_port)/Raw(load=data)
        packet = IP(raw(packet))

        send(packet)
