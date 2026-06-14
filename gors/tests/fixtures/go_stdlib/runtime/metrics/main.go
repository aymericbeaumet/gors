package main

import (
	"fmt"
	"runtime/metrics"
)

func main() {
	fmt.Println(metrics.KindUint64 == metrics.KindUint64, metrics.KindBad == metrics.KindFloat64)
}
