local raw = redis.call('HGET', KEYS[1], ARGV[1])
if not raw then return {0} end
local head = cjson.decode(raw)
local key = ARGV[2] .. head.version .. ':' .. ARGV[3]
local record = redis.call('HGET', key, 'record')
if not record then return {0} end
local anchor = cjson.decode(record)
local conversation = redis.call('HGET', key, 'conversation')
local context = tonumber(ARGV[4])
local query = '@version:{' .. head.version .. '} @conversation:{' .. conversation .. '} @ordinal:['
    .. tostring(anchor.ordinal - context) .. ' ' .. tostring(anchor.ordinal + context) .. ']'
return redis.call('FT.SEARCH', KEYS[2], query, 'RETURN', 1, 'record', 'SORTBY', 'ordinal', 'ASC',
    'LIMIT', 0, 2001, 'TIMEOUT', 30000, 'DIALECT', 2)
