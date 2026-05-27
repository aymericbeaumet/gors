package main

import (
	"encoding/xml"
	"fmt"
)

func main() {
	fmt.Println(len(xml.Header), xml.Header[:5])
}
