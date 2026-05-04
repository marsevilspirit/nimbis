package tests

import "fmt"

func hashKey(key string) uint64 {
	var hasher uint64 = 0xcbf29ce484222325
	for i := 0; i < len(key); i++ {
		hasher ^= uint64(key[i])
		hasher *= 0x100000001b3
	}
	return hasher
}

func findCrossShardKeys(workerCount int) (string, string) {
	seen := map[int]string{}
	for i := 0; i < 2000; i++ {
		key := fmt.Sprintf("e2e:route:key:%d", i)
		worker := int(hashKey(key) % uint64(workerCount))
		if existing, ok := seen[worker]; ok && existing != key {
			for otherWorker, otherKey := range seen {
				if otherWorker != worker {
					return otherKey, key
				}
			}
		}
		if _, ok := seen[worker]; !ok {
			seen[worker] = key
		}
	}
	panic("failed to find cross-shard keys")
}
