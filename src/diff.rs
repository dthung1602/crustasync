/*

build table {hash -> node}
async loop though target tree
    if hash found:
        name the same: do nothing
        name diff: rename (aka mv)
    else:
        is file: upload
        is dir:
            create empty dir
            recursive sync tree
    pop hash/node from table

sort remaining items in table from bottom to top
loop though levels of remaining items
    async delete items levels
 */
