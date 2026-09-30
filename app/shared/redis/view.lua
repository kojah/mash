local raw = redis.call('HGET', KEYS[1], ARGV[1])
if not raw then return {0} end
local head = cjson.decode(raw)
local key = ARGV[2] .. head.version .. ':' .. ARGV[3]
local record = redis.call('HGET', key, 'record')
if not record then return {0} end
local anchor = cjson.decode(record)
local conversation = redis.call('HGET', key, 'conversation')
local context = tonumber(ARGV[4])
if anchor.context and anchor.context.kind ~= 'linear' then
    local scope = '@version:{' .. head.version .. '} @conversation:{' .. conversation .. '} '
    local result = {0}
    local function add(row)
        if #row < 3 then return nil end
        result[#result + 1] = row[2]
        result[#result + 1] = row[3]
        result[1] = result[1] + 1
        return cjson.decode(row[3][2])
    end
    add({1,key,{'record',record}})
    local current = anchor
    for _ = 1, context do
        if not current.context or current.context.kind ~= 'parent' then break end
        local ordinal = tostring(current.context.ordinal)
        local row = redis.call('FT.SEARCH', KEYS[2], scope .. '@ordinal:[' .. ordinal .. ' ' .. ordinal .. ']',
            'RETURN', 1, 'record', 'LIMIT', 0, 1, 'TIMEOUT', 30000, 'DIALECT', 2)
        current = add(row)
        if not current then break end
    end
    current = anchor
    for _ = 1, context do
        local ordinal = tostring(current.ordinal)
        local row = redis.call('FT.SEARCH', KEYS[2], scope .. '@context_parent:[' .. ordinal .. ' ' .. ordinal .. ']',
            'RETURN', 1, 'record', 'SORTBY', 'ordinal', 'ASC', 'LIMIT', 0, 1, 'TIMEOUT', 30000, 'DIALECT', 2)
        current = add(row)
        if not current then break end
    end
    return result
end
local query = '@version:{' .. head.version .. '} @conversation:{' .. conversation .. '} @ordinal:['
    .. tostring(anchor.ordinal - context) .. ' ' .. tostring(anchor.ordinal + context) .. ']'
return redis.call('FT.SEARCH', KEYS[2], query, 'RETURN', 1, 'record', 'SORTBY', 'ordinal', 'ASC',
    'LIMIT', 0, 2001, 'TIMEOUT', 30000, 'DIALECT', 2)
