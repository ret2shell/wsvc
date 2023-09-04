# 架构想法

## 布局

wsvc采用类似git的想法，使用`blob`来存储数据，但是压缩算法比较唯一，使用`DEFLATE`。

```text
- .wsvc : 数据文件夹
  - objects : 数据对象目录
    - xxxxxx : 数据对象
  - trees : 树状结构目录
    - xxxxxx : 树状结构，json保存的目录结构
  - records : 记录目录
    - xxxxxx : 记录，json保存的记录
  - HEAD : 最新的记录
```

`records`里保存信息和时间等，同时保存一个这些信息的hash作为`ObjectId`。在信息中，还要留存根目录的树状结构`ObjectId`。

在`checkout`时，通过`Record`中根目录的`ObjectId`索引到对应的树状结构`Tree`，然后再递归索引到对应的`Blob`，并将压缩后的数据还原。

反向同理。

## 冲突解决

在wsvc中不存在冲突问题，每次建立`Record`都会基于时间戳对整个目录进行完整扫描，如果对应的数据文件`ObjectId`已经存在，那么就不会再次创建。多人协作时始终以最新的`Record`为准，即使有较旧的`Record`，在同步时仅会将其插入到历史记录中，不会改变`HEAD`所指向的东西。
