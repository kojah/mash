local heads = redis.call('HVALS', KEYS[1])
local versions = {}
for _, value in ipairs(heads) do
    local head = cjson.decode(value)
    if head.counts.searchable_messages ~= head.counts.embeddings or
        (head.counts.searchable_messages > 0 and head.generation ~= ARGV[4]) then
        return redis.error_reply('MASH_INCOMPATIBLE')
    end
    table.insert(versions, head.version)
end
if #versions == 0 then return {{0}, {0}} end
local active = '@version:{' .. table.concat(versions, '|') .. '}'
local lexical = {0}
if ARGV[1] ~= '' then
    lexical = redis.call('FT.SEARCH', KEYS[2], '(' .. ARGV[1] .. ') ' .. active .. ARGV[2],
        'WITHSCORES', 'VERBATIM', 'NOSTOPWORDS', 'RETURN', 4, 'record', 'node_id', 'provider', 'projection',
        'LIMIT', 0, 50, 'TIMEOUT', 30000, 'DIALECT', 2)
end
local query = '(' .. active .. ' @searchable:[1 1]' .. ARGV[2] .. ')=>[KNN 50 @vector $BLOB AS distance]'
local semantic = redis.call('FT.SEARCH', KEYS[2], query, 'WITHSCORES',
    'RETURN', 5, 'record', 'node_id', 'provider', 'projection', 'distance',
    'SORTBY', 'distance', 'ASC', 'LIMIT', 0, 50, 'TIMEOUT', 30000,
    'PARAMS', 2, 'BLOB', ARGV[3], 'DIALECT', 2)
return {lexical, semantic}
