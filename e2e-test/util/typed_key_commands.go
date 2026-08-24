package util

import (
	"context"
	"time"

	"github.com/redis/go-redis/v9"
)

// KeyType selects the one physical type database touched by a key lifecycle
// command. Nimbis deliberately requires this selector instead of discovering
// a key's type across all databases.
type KeyType string

const (
	StringType KeyType = "STRING"
	HashType   KeyType = "HASH"
	ListType   KeyType = "LIST"
	SetType    KeyType = "SET"
	ZSetType   KeyType = "ZSET"
)

func Del(ctx context.Context, client *redis.Client, keyType KeyType, keys ...string) *redis.IntCmd {
	args := typedArgs("DEL", keyType, keys)
	cmd := redis.NewIntCmd(ctx, args...)
	_ = client.Process(ctx, cmd)
	return cmd
}

func Exists(ctx context.Context, client *redis.Client, keyType KeyType, keys ...string) *redis.IntCmd {
	args := typedArgs("EXISTS", keyType, keys)
	cmd := redis.NewIntCmd(ctx, args...)
	_ = client.Process(ctx, cmd)
	return cmd
}

func Expire(
	ctx context.Context,
	client *redis.Client,
	keyType KeyType,
	key string,
	expiration time.Duration,
) *redis.BoolCmd {
	cmd := redis.NewBoolCmd(ctx, "EXPIRE", string(keyType), key, int64(expiration/time.Second))
	_ = client.Process(ctx, cmd)
	return cmd
}

func TTL(ctx context.Context, client *redis.Client, keyType KeyType, key string) *redis.DurationCmd {
	cmd := redis.NewDurationCmd(ctx, time.Second, "TTL", string(keyType), key)
	_ = client.Process(ctx, cmd)
	return cmd
}

func typedArgs(command string, keyType KeyType, keys []string) []interface{} {
	args := make([]interface{}, 0, len(keys)+2)
	args = append(args, command, string(keyType))
	for _, key := range keys {
		args = append(args, key)
	}
	return args
}
