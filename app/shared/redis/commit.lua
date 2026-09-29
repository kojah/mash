-- All canonical records and searchable documents are staged first. This script
-- changes the visible revision only if the observed node metadata still matches.
local current = redis.call('HGET', KEYS[1], ARGV[1]) or ''
if current ~= ARGV[2] then return redis.error_reply('MASH_CONFLICT') end
for i = 2, #KEYS do
    if redis.call('EXISTS', KEYS[i]) ~= 1 then
        return redis.error_reply('MASH_STAGE_MISSING')
    end
end
local next = cjson.decode(ARGV[3])
local retired = {}
if ARGV[6] ~= '' and redis.call('TYPE', ARGV[6]).ok == 'list' then
    retired = redis.call('LRANGE', ARGV[6], 0, -1)
end
local time = redis.call('TIME')
local now = time[1] .. '.' .. string.format('%06d', tonumber(time[2]))
if ARGV[4] == '1' then next.last_synced_at = now end
if ARGV[5] == '1' then next.last_updated_at = now end
for i = 2, #KEYS do redis.call('PERSIST', KEYS[i]) end
redis.call('HSET', KEYS[1], ARGV[1], cjson.encode(next))
for _, key in ipairs(retired) do redis.call('UNLINK', key) end
if ARGV[6] ~= '' then redis.call('UNLINK', ARGV[6]) end
return 1
