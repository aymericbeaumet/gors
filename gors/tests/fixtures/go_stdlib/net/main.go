package main

import (
	"fmt"
	"net"
)

func main() {
	fmt.Println(net.IPv4len, net.IPv6len)
}
