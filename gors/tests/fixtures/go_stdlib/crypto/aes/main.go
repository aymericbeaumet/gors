package main

import (
	"crypto/aes"
	"fmt"
)

func caseAesConstantsAndErrors() {
	// gors:stdlib-cover crypto/aes::BlockSize
	fmt.Println(aes.BlockSize)
	// gors:stdlib-cover crypto/aes::KeySizeError
	// gors:stdlib-cover crypto/aes::KeySizeError.Error
	fmt.Println(aes.KeySizeError.Error(aes.KeySizeError(7)))
}

func caseAesNewCipher() {
	// gors:stdlib-cover crypto/aes::NewCipher
	bad, badErr := aes.NewCipher([]byte{1, 2, 3})
	fmt.Println("newcipher-bad", bad == nil, badErr != nil, badErr.Error())

	key := []byte{
		0x00, 0x01, 0x02, 0x03,
		0x04, 0x05, 0x06, 0x07,
		0x08, 0x09, 0x0a, 0x0b,
		0x0c, 0x0d, 0x0e, 0x0f,
	}
	plain := []byte{
		0x00, 0x11, 0x22, 0x33,
		0x44, 0x55, 0x66, 0x77,
		0x88, 0x99, 0xaa, 0xbb,
		0xcc, 0xdd, 0xee, 0xff,
	}
	block, err := aes.NewCipher(key)
	ciphertext := make([]byte, aes.BlockSize)
	block.Encrypt(ciphertext, plain)
	roundtrip := make([]byte, aes.BlockSize)
	block.Decrypt(roundtrip, ciphertext)
	fmt.Println(
		"newcipher-ok",
		err == nil,
		block.BlockSize(),
		len(ciphertext),
		int(ciphertext[0]),
		int(ciphertext[1]),
		int(ciphertext[2]),
		int(ciphertext[3]),
		int(ciphertext[4]),
		int(ciphertext[5]),
		int(ciphertext[6]),
		int(ciphertext[7]),
		int(ciphertext[8]),
		int(ciphertext[9]),
		int(ciphertext[10]),
		int(ciphertext[11]),
		int(ciphertext[12]),
		int(ciphertext[13]),
		int(ciphertext[14]),
		int(ciphertext[15]),
	)
	fmt.Println(
		"newcipher-roundtrip",
		int(roundtrip[0]),
		int(roundtrip[1]),
		int(roundtrip[14]),
		int(roundtrip[15]),
	)
}

func main() {
	caseAesConstantsAndErrors()
	caseAesNewCipher()
}
