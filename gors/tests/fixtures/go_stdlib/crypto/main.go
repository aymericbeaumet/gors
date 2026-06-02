package main

import (
	"crypto"
	"fmt"
)

func caseCryptoHashConstants() {
	// gors:stdlib-cover crypto::Hash
	// gors:stdlib-cover crypto::BLAKE2b_256
	// gors:stdlib-cover crypto::BLAKE2b_384
	// gors:stdlib-cover crypto::BLAKE2b_512
	// gors:stdlib-cover crypto::BLAKE2s_256
	// gors:stdlib-cover crypto::MD4
	// gors:stdlib-cover crypto::MD5
	// gors:stdlib-cover crypto::MD5SHA1
	// gors:stdlib-cover crypto::RIPEMD160
	// gors:stdlib-cover crypto::SHA1
	// gors:stdlib-cover crypto::SHA224
	// gors:stdlib-cover crypto::SHA256
	// gors:stdlib-cover crypto::SHA384
	// gors:stdlib-cover crypto::SHA3_224
	// gors:stdlib-cover crypto::SHA3_256
	// gors:stdlib-cover crypto::SHA3_384
	// gors:stdlib-cover crypto::SHA3_512
	// gors:stdlib-cover crypto::SHA512
	// gors:stdlib-cover crypto::SHA512_224
	// gors:stdlib-cover crypto::SHA512_256
	fmt.Println(
		int(crypto.MD4),
		int(crypto.MD5),
		int(crypto.SHA1),
		int(crypto.SHA224),
		int(crypto.SHA256),
		int(crypto.SHA384),
		int(crypto.SHA512),
		int(crypto.MD5SHA1),
		int(crypto.RIPEMD160),
		int(crypto.SHA3_224),
		int(crypto.SHA3_256),
		int(crypto.SHA3_384),
		int(crypto.SHA3_512),
		int(crypto.SHA512_224),
		int(crypto.SHA512_256),
		int(crypto.BLAKE2s_256),
		int(crypto.BLAKE2b_256),
		int(crypto.BLAKE2b_384),
		int(crypto.BLAKE2b_512),
	)
}

func caseCryptoHashMethods() {
	// gors:stdlib-cover crypto::Hash.HashFunc
	// gors:stdlib-cover crypto::Hash.Size
	// gors:stdlib-cover crypto::Hash.String
	fmt.Println(
		crypto.SHA256.HashFunc() == crypto.SHA256,
		crypto.SHA256.Size(),
		crypto.SHA256.String(),
	)
}

func main() {
	caseCryptoHashConstants()
	caseCryptoHashMethods()
}
