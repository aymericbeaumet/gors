package main

import (
	"fmt"
	"log"
)

func main() {
	fmt.Println(log.Ldate, log.Ltime, log.Lshortfile, log.LstdFlags)
}
