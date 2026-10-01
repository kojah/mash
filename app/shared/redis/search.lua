local heads = redis.call('HGETALL', KEYS[1])
local versions = {}
for i = 1, #heads, 2 do
  if heads[i] ~= ARGV[5] then
    local head = cjson.decode(heads[i + 1])
    if head.counts.searchable_messages ~= head.counts.embeddings or
        (head.counts.searchable_messages > 0 and head.generation ~= ARGV[4]) then
        return redis.error_reply('MASH_INCOMPATIBLE')
    end
    table.insert(versions, head.version)
  end
end
if #versions == 0 then return {{0}, {0}} end
local active = '@version:{' .. table.concat(versions, '|') .. '}'
local lexical = {0}
if ARGV[1] ~= '' then
    lexical = redis.call('FT.SEARCH', KEYS[2], '(' .. ARGV[1] .. ') ' .. active .. ARGV[2],
        'WITHSCORES', 'VERBATIM', 'NOSTOPWORDS', 'RETURN', 5, 'record', 'node_id', 'provider', 'projection', 'conversation_record',
        'LIMIT', 0, tonumber(ARGV[6]), 'TIMEOUT', 30000, 'DIALECT', 2)
end
local query = '(' .. active .. ' @searchable:[1 1]' .. ARGV[2] .. ')=>[KNN ' .. ARGV[6] .. ' @vector $BLOB AS distance]' .. ARGV[7]
local semantic = redis.call('FT.SEARCH', KEYS[2], query, 'WITHSCORES',
    'RETURN', 6, 'record', 'node_id', 'provider', 'projection', 'distance', 'conversation_record',
    'SORTBY', 'distance', 'ASC', 'LIMIT', 0, tonumber(ARGV[6]), 'TIMEOUT', 30000,
    'PARAMS', 2, 'BLOB', ARGV[3], 'DIALECT', 2)
return {lexical, semantic}
