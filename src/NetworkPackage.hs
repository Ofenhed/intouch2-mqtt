module NetworkPackage where

import qualified Data.ByteString.Char8 as C8

data NetworkPackageData = NetPing
                        | NetPong
                        | NetUnknownPackage C8.ByteString
                        deriving (Eq)

data NetworkPackage = AuthorizedNetworkPackage { networkPackageSrc :: Maybe C8.ByteString
                                               , networkPackageDst :: Maybe C8.ByteString
                                               , networkPackageContent :: NetworkPackageData }
                    | HelloPackage C8.ByteString
                    deriving (Eq)

instance Show NetworkPackageData where
  show NetPing = "APING"
  show NetPong = "APING."
  show (NetUnknownPackage x) = C8.unpack x

instance Show NetworkPackage where
  show (AuthorizedNetworkPackage src dst content) = "<PACKT>" ++ (case src of Nothing -> "" ; Just x -> "<SRCCN>" ++ (C8.unpack x) ++ "</SRCCN>")
                                                              ++ (case dst of Nothing -> "" ; Just x -> "<DESCN>" ++ (C8.unpack x) ++ "</DESCN>")
                                                              ++ "<DATAS>" ++ (show content) ++ "</DATAS></PACKT>"
  show (HelloPackage h) = "<HELLO>" ++ (C8.unpack h) ++ "</HELLO>"
