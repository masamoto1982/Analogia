# MONIAC 風の国民経済モデル。水 = お金。
# タンクに貯まり、パイプを流れ、全体の水量はパイプが外界に触れない限り変わらない。

# 政策パラメータ（ユーザーワード。Dictionary パネルに並ぶ）
[ 0.25 ] 'TAX-RATE' DEF
[ 0.15 ] 'SAVING-RATE' DEF
[ 0.08 ] 'IMPORT-RATE' DEF

# タンク（ストック）
400 'FIRMS' TANK          # 企業：生産と売上
200 'HOUSEHOLDS' TANK     # 家計：手元の所得
100 'TREASURY' TANK       # 国庫
200 'SAVINGS' TANK        # 遊休残高（貯蓄）
100 'FOREIGN' TANK        # 海外

# パイプ（フロー）：[ 流量 ] '流出元' '流入先' '名前' PIPE
[ FIRMS 0.5 MUL ]                 'FIRMS'      'HOUSEHOLDS' 'INCOME'      PIPE
[ INCOME TAX-RATE MUL ]           'HOUSEHOLDS' 'TREASURY'   'TAX'         PIPE
[ HOUSEHOLDS SAVING-RATE MUL ]    'HOUSEHOLDS' 'SAVINGS'    'SAVING'      PIPE
[ HOUSEHOLDS IMPORT-RATE MUL ]    'HOUSEHOLDS' 'FOREIGN'    'IMPORTS'     PIPE
[ HOUSEHOLDS 0.6 MUL ]            'HOUSEHOLDS' 'FIRMS'      'CONSUMPTION' PIPE
[ TREASURY 0.3 MUL ]              'TREASURY'   'FIRMS'      'SPENDING'    PIPE
[ FOREIGN 0.2 MUL ]               'FOREIGN'    'FIRMS'      'EXPORTS'     PIPE
# 投資：遊休残高が多いほど金利が下がり、投資の割合が上がる（MONIAC のカム）
[ SAVINGS DUP [ 0 0.02 200 0.1 400 0.2 ] CAM MUL ] 'SAVINGS' 'FIRMS' 'INVESTMENT' PIPE

# 50 期間流す
50 0.1 RUN

# 減税してさらに 50 期間。流れは止めずに続きから
[ 0.1 ] 'TAX-RATE' DEF
50 0.1 RUN
