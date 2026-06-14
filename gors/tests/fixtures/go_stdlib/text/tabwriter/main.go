package main

import (
	"fmt"
	"text/tabwriter"
)

func main() {
	fmt.Println(tabwriter.AlignRight, tabwriter.DiscardEmptyColumns, tabwriter.Debug)
}
