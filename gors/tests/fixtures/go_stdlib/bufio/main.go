package main

import (
	"bufio"
	"fmt"
)

func main() {
	fmt.Println("== bufio/constants ==")
	caseConstants()
	fmt.Println("== bufio/errors ==")
	caseErrors()
}

func caseConstants() {
	// gors:stdlib-cover bufio::MaxScanTokenSize
	fmt.Println(bufio.MaxScanTokenSize)
}

func caseErrors() {
	// gors:stdlib-cover bufio::ErrAdvanceTooFar bufio::ErrBadReadCount bufio::ErrBufferFull bufio::ErrFinalToken bufio::ErrInvalidUnreadByte bufio::ErrInvalidUnreadRune bufio::ErrNegativeAdvance bufio::ErrNegativeCount bufio::ErrTooLong
	fmt.Println(bufio.ErrAdvanceTooFar.Error())
	fmt.Println(bufio.ErrBadReadCount.Error())
	fmt.Println(bufio.ErrBufferFull.Error())
	fmt.Println(bufio.ErrFinalToken.Error())
	fmt.Println(bufio.ErrInvalidUnreadByte.Error())
	fmt.Println(bufio.ErrInvalidUnreadRune.Error())
	fmt.Println(bufio.ErrNegativeAdvance.Error())
	fmt.Println(bufio.ErrNegativeCount.Error())
	fmt.Println(bufio.ErrTooLong.Error())
}
